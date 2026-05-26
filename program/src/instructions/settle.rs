//! Shared settlement core for both authorization paths (`SubmitBatch` committee-verified and
//! `SubmitBatchTrusted` gateway-authority-verified). Authentication happens in each handler; this
//! function owns the deterministic, never-trust-the-header settlement:
//!
//! - bind the frame to the market + replay guard,
//! - re-derive the residual economics from the fills (header is advisory),
//! - Tier-1 P2P at `$0.50` + Tier-2 PropAMM residual at the clearing price,
//! - mutate each `UserPosition`, update market aggregates + MM backstop reservation + epoch.
//!
//! No tokens move — settlement is a pure ledger update; full collateralization is maintained by the
//! MM backstop reservation (the vault already holds the funds). See `submit_batch.rs` for the trust
//! model and collateralization rationale.

use {
    crate::{
        error::ShadowError,
        math,
        state::{cast, cast_mut, require_owned_by_program, MarketState, UserPosition},
        utils::{require_pda, require_writable},
    },
    pinocchio::{account::AccountView, error::ProgramError, ProgramResult},
    protocol::{
        ids::seeds, read_fill, validate_frame_len, DIRECTION_NO_HEAVY, DIRECTION_YES_HEAVY,
        MAX_FILLS, MIDPOINT_PRICE, SCALE_FACTOR,
    },
};

/// Apply a validated batch frame to `market_ai` + the per-fill `position_accounts`. The caller has
/// already authenticated the batch (committee signatures or trusted authority) and verified the
/// market is owned by this program.
pub(crate) fn apply_settlement(
    market_ai: &mut AccountView,
    position_accounts: &mut [AccountView],
    data: &[u8],
) -> ProgramResult {
    // Parse + length-validate the frame (header ++ fills, exact size).
    let header = validate_frame_len(data).ok_or(ShadowError::InvalidInstructionData)?;
    let fill_count = header.fill_count as usize;
    if fill_count > MAX_FILLS {
        return Err(ShadowError::TooManyFills.into());
    }
    if position_accounts.len() < fill_count {
        return Err(ProgramError::NotEnoughAccountKeys);
    }

    require_owned_by_program(market_ai)?;
    let market_key = *market_ai.address().as_array();

    // Bind frame to this market + replay guard; read pricing params.
    let (base_oracle_price, max_skew_premium, imbalance_threshold) = {
        let md = market_ai.try_borrow()?;
        let m: &MarketState = cast(&md)?;
        if header.market != market_key {
            return Err(ShadowError::FrameEconomicsMismatch.into());
        }
        // Lifecycle gate: batches only settle while trading is open.
        if !m.is_trading() {
            return Err(ShadowError::TradingClosed.into());
        }
        if header.epoch <= m.last_epoch {
            return Err(ShadowError::StaleEpoch.into());
        }
        (m.base_oracle_price, m.max_skew_premium, m.imbalance_threshold)
    };

    // Re-derive economics from the fills; the header is advisory only.
    let mut sum_res_yes: u64 = 0;
    let mut sum_res_no: u64 = 0;
    for i in 0..fill_count {
        let f = read_fill(data, i).ok_or(ShadowError::InvalidInstructionData)?;
        sum_res_yes = math::add(sum_res_yes, f.residual_yes)?;
        sum_res_no = math::add(sum_res_no, f.residual_no)?;
    }
    let (heavy_sum, light_sum) = match header.direction {
        DIRECTION_YES_HEAVY => (sum_res_yes, sum_res_no),
        DIRECTION_NO_HEAVY => (sum_res_no, sum_res_yes),
        _ => return Err(ShadowError::FrameEconomicsMismatch.into()),
    };
    if light_sum != 0 || heavy_sum != header.net_imbalance {
        return Err(ShadowError::FrameEconomicsMismatch.into());
    }

    // Tier-2 price + MM backstop obligation.
    let clear_price = math::clearing_price(
        header.net_imbalance,
        header.direction,
        base_oracle_price,
        max_skew_premium,
        imbalance_threshold,
    )?;
    let heavy_price = match header.direction {
        DIRECTION_YES_HEAVY => clear_price,
        _ => SCALE_FACTOR - clear_price, // clear_price <= MAX_PRICE < SCALE_FACTOR
    };
    let mm_obligation = math::collateral_for(header.net_imbalance, SCALE_FACTOR - heavy_price)?;

    // Apply per-fill settlement (bounded loop, fully checked).
    let mut add_yes: u64 = 0;
    let mut add_no: u64 = 0;
    for i in 0..fill_count {
        let f = read_fill(data, i).ok_or(ShadowError::InvalidInstructionData)?;
        let position_ai = &mut position_accounts[i];
        require_writable(position_ai)?;
        require_owned_by_program(position_ai)?;

        let bump = {
            let pd = position_ai.try_borrow()?;
            let pos: &UserPosition = cast(&pd)?;
            if pos.owner != f.user || pos.market != market_key {
                return Err(ShadowError::InvalidAccount.into());
            }
            pos.bump
        };
        require_pda(position_ai, &[seeds::POSITION, &market_key, &f.user], bump, &crate::ID)?;

        let p2p_total = math::add(f.p2p_yes, f.p2p_no)?;
        let t1_cost = math::collateral_for(p2p_total, MIDPOINT_PRICE)?;
        let (res_yes, res_no, t2_cost) = match header.direction {
            DIRECTION_YES_HEAVY => (f.residual_yes, 0, math::collateral_for(f.residual_yes, heavy_price)?),
            _ => (0, f.residual_no, math::collateral_for(f.residual_no, heavy_price)?),
        };

        let total_cost = math::add(t1_cost, t2_cost)?;
        let yes_add = math::add(f.p2p_yes, res_yes)?;
        let no_add = math::add(f.p2p_no, res_no)?;

        {
            let mut pd = position_ai.try_borrow_mut()?;
            let pos: &mut UserPosition = cast_mut(&mut pd)?;
            if pos.collateral < total_cost {
                return Err(ShadowError::InsufficientCollateral.into());
            }
            pos.collateral -= total_cost;
            pos.yes_qty = math::add(pos.yes_qty, yes_add)?;
            pos.no_qty = math::add(pos.no_qty, no_add)?;
        }

        add_yes = math::add(add_yes, yes_add)?;
        add_no = math::add(add_no, no_add)?;
    }

    // Update market aggregates + MM backstop reservation + epoch.
    {
        let mut md = market_ai.try_borrow_mut()?;
        let m: &mut MarketState = cast_mut(&mut md)?;
        m.total_yes_supply = math::add(m.total_yes_supply, add_yes)?;
        m.total_no_supply = math::add(m.total_no_supply, add_no)?;
        match header.direction {
            DIRECTION_YES_HEAVY => m.mm_no = math::add(m.mm_no, header.net_imbalance)?,
            _ => m.mm_yes = math::add(m.mm_yes, header.net_imbalance)?,
        }
        if m.mm_collateral < mm_obligation {
            return Err(ShadowError::InsufficientMmCollateral.into());
        }
        m.mm_collateral = math::sub(m.mm_collateral, mm_obligation)?;
        m.last_epoch = header.epoch;
    }

    Ok(())
}
