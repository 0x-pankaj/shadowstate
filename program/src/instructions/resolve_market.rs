//! `ResolveMarket` (disc 7): the resolver authority declares the final outcome. After this, winning
//! contracts pay `$1` and losing contracts pay `$0` (see `ClaimWinnings` / `ClaimMmWinnings`).
//! Outcomes are final — a resolved market cannot be re-resolved.
//!
//! The authority here is `market.authority` (the same key that tunes risk params). A production
//! deployment would route this through an oracle / optimistic-resolution module; that module would
//! become the authority. The valid outcomes are `OUTCOME_YES_WON` and `OUTCOME_NO_WON`.
//!
//! Accounts:
//! 0. `[signer]`   authority (== market.authority)
//! 1. `[writable]` market_state PDA
//!
//! Data: `outcome: u8` (`OUTCOME_YES_WON` | `OUTCOME_NO_WON`).

use {
    crate::{
        error::ShadowError,
        state::{cast_mut, require_owned_by_program, MarketState},
        utils::{require_signer, require_writable},
    },
    pinocchio::{account::AccountView, error::ProgramError, ProgramResult},
    protocol::{OUTCOME_INVALID, OUTCOME_NO_WON, OUTCOME_UNRESOLVED, OUTCOME_YES_WON},
};

pub fn process(accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    let outcome = *data.first().ok_or(ShadowError::InvalidInstructionData)?;
    if outcome != OUTCOME_YES_WON && outcome != OUTCOME_NO_WON && outcome != OUTCOME_INVALID {
        return Err(ShadowError::InvalidOutcome.into());
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
    // Lifecycle gate: a market must be closed (trading ended) before it can be resolved.
    if !m.is_closed() {
        return Err(ShadowError::MarketNotClosed.into());
    }
    if m.outcome != OUTCOME_UNRESOLVED {
        return Err(ShadowError::MarketAlreadyResolved.into());
    }

    m.outcome = outcome;
    Ok(())
}
