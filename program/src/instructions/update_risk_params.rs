//! `UpdateRiskParams` (disc 4): the MM admin retunes pricing params as volatility shifts. This is
//! the on-chain target of the `mm-gateway` "parameter modification portal". The committee is NOT
//! touchable here — it is immutable after `InitializeMarket`.
//!
//! Accounts:
//! 0. `[signer]`          authority (== market.authority)
//! 1. `[writable]`        market_state PDA
//!
//! Data: `base_oracle_price:u64 | max_skew_premium:u64 | imbalance_threshold:u64`.

use {
    crate::{
        error::ShadowError,
        state::{cast_mut, require_owned_by_program, MarketState},
        utils::{require_signer, require_writable},
    },
    pinocchio::{account::AccountView, error::ProgramError, ProgramResult},
    protocol::{MAX_PRICE, MIN_PRICE},
};

pub fn process(accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    if data.len() < 24 {
        return Err(ShadowError::InvalidInstructionData.into());
    }
    let base_oracle_price = u64::from_le_bytes(data[0..8].try_into().unwrap());
    let max_skew_premium = u64::from_le_bytes(data[8..16].try_into().unwrap());
    let imbalance_threshold = u64::from_le_bytes(data[16..24].try_into().unwrap());

    if imbalance_threshold == 0
        || base_oracle_price < MIN_PRICE
        || base_oracle_price > MAX_PRICE
        || max_skew_premium > MAX_PRICE
    {
        return Err(ShadowError::InvalidRiskParams.into());
    }

    let [authority, market_ai, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    require_signer(authority)?;
    require_writable(market_ai)?;
    require_owned_by_program(market_ai)?;

    let mut md = market_ai.try_borrow_mut()?;
    let m: &mut MarketState = cast_mut(&mut md)?;
    if &m.authority != authority.address().as_array() {
        return Err(ShadowError::MissingSignature.into());
    }

    m.base_oracle_price = base_oracle_price;
    m.max_skew_premium = max_skew_premium;
    m.imbalance_threshold = imbalance_threshold;

    Ok(())
}
