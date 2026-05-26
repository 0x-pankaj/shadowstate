//! `ClaimWinnings` (disc 8): after resolution, a user redeems their winning contracts for `$1` each.
//! `TransferChecked` moves the payout from the vault to the user (signed by the vault PDA) and zeroes
//! both position legs (the winning leg is paid; the losing leg is worthless). Undeployed
//! `position.collateral` is untouched — withdraw it separately via `WithdrawCollateral`.
//!
//! Payout = `winning_qty × $1` = `collateral_for(winning_qty, SCALE_FACTOR)` (one base unit per
//! contract). The vault is solvent for this because every contract was fully collateralized to `$1`
//! at settlement (P2P pairs + the reserved MM backstop).
//!
//! Accounts:
//! 0. `[signer]`   owner (user)
//! 1. `[]`         market_state PDA
//! 2. `[writable]` position PDA
//! 3. `[writable]` user token account (dest)
//! 4. `[writable]` vault token account (source; == market.vault)
//! 5. `[]`         collateral mint (== market.collateral_mint)
//! 6. `[]`         vault authority PDA `[b"vault", market]`
//! 7. `[]`         Token-2022 program

use {
    crate::{
        error::ShadowError,
        math,
        state::{cast, cast_mut, require_owned_by_program, MarketState, UserPosition},
        utils::{require_address, require_pda, require_signer, require_writable},
    },
    pinocchio::{
        account::AccountView,
        address::Address,
        cpi::{Seed, Signer},
        error::ProgramError,
        ProgramResult,
    },
    pinocchio_token_2022::{instructions::TransferChecked, state::Mint},
    protocol::{ids::seeds, MIDPOINT_PRICE, OUTCOME_INVALID, OUTCOME_NO_WON, OUTCOME_YES_WON, SCALE_FACTOR},
};

pub fn process(accounts: &mut [AccountView], _data: &[u8]) -> ProgramResult {
    let [owner, market_ai, position_ai, user_token, vault, mint_ai, vault_authority, token_program, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    require_signer(owner)?;
    require_writable(position_ai)?;
    require_writable(user_token)?;
    require_writable(vault)?;
    require_address(token_program, &pinocchio_token_2022::ID)?;
    require_owned_by_program(market_ai)?;

    let market_key = *market_ai.address().as_array();
    let owner_key = *owner.address().as_array();

    // Read market outcome + bind vault/mint/authority.
    let (outcome, decimals, vault_bump) = {
        let md = market_ai.try_borrow()?;
        let m: &MarketState = cast(&md)?;
        require_address(vault, &Address::new_from_array(m.vault))?;
        require_address(mint_ai, &Address::new_from_array(m.collateral_mint))?;
        require_pda(vault_authority, &[seeds::VAULT, &market_key], m.vault_bump, &crate::ID)?;
        (m.outcome, Mint::from_account_view(mint_ai)?.decimals(), m.vault_bump)
    };
    if outcome != OUTCOME_YES_WON && outcome != OUTCOME_NO_WON && outcome != OUTCOME_INVALID {
        return Err(ShadowError::MarketNotResolved.into());
    }

    // Validate position ownership + compute the payout on the winning leg.
    require_owned_by_program(position_ai)?;
    let payout = {
        let pd = position_ai.try_borrow()?;
        let pos: &UserPosition = cast(&pd)?;
        if pos.owner != owner_key || pos.market != market_key {
            return Err(ShadowError::InvalidAccount.into());
        }
        require_pda(position_ai, &[seeds::POSITION, &market_key, &owner_key], pos.bump, &crate::ID)?;
        match outcome {
            OUTCOME_YES_WON => math::collateral_for(pos.yes_qty, SCALE_FACTOR)?,
            OUTCOME_NO_WON => math::collateral_for(pos.no_qty, SCALE_FACTOR)?,
            // Voided market: both legs settle at the $0.50 midpoint (solvent: yes_total == no_total).
            _ => math::add(
                math::collateral_for(pos.yes_qty, MIDPOINT_PRICE)?,
                math::collateral_for(pos.no_qty, MIDPOINT_PRICE)?,
            )?,
        }
    };
    if payout == 0 {
        return Err(ShadowError::NothingToClaim.into());
    }

    // Pay out vault -> user, signed by the vault PDA.
    TransferChecked {
        from: vault,
        mint: mint_ai,
        to: user_token,
        authority: vault_authority,
        amount: payout,
        decimals,
        token_program: &pinocchio_token_2022::ID,
    }
    .invoke_signed(&[Signer::from(&[
        Seed::from(seeds::VAULT),
        Seed::from(&market_key),
        Seed::from(&[vault_bump]),
    ])])?;

    // Both legs are now settled: zero them so the position cannot be claimed twice.
    let mut pd = position_ai.try_borrow_mut()?;
    let pos: &mut UserPosition = cast_mut(&mut pd)?;
    pos.yes_qty = 0;
    pos.no_qty = 0;

    Ok(())
}
