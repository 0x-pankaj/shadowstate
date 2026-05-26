//! `CloseMarket` (disc 11): the authority ends the trading window. After this no further batches
//! settle (`SubmitBatch`/`SubmitBatchTrusted` require `STATUS_TRADING`) and the market becomes
//! resolvable (`ResolveMarket` requires `STATUS_CLOSED`). One-way: a closed market cannot reopen.
//!
//! Accounts:
//! 0. `[signer]`   authority (== market.authority)
//! 1. `[writable]` market_state PDA

use {
    crate::{
        error::ShadowError,
        state::{cast_mut, require_owned_by_program, MarketState},
        utils::{require_signer, require_writable},
    },
    pinocchio::{account::AccountView, error::ProgramError, ProgramResult},
    protocol::STATUS_CLOSED,
};

pub fn process(accounts: &mut [AccountView], _data: &[u8]) -> ProgramResult {
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
    // Idempotency guard: only an open market can be closed (no reopen, no double-close).
    if !m.is_trading() {
        return Err(ShadowError::TradingClosed.into());
    }

    m.status = STATUS_CLOSED;
    Ok(())
}
