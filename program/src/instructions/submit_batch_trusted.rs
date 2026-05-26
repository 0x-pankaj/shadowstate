//! `SubmitBatchTrusted` (disc 10): the **gateway-authority-verified** settlement — the Arcium path.
//!
//! Under real Arcium the order book is matched inside the MXE and the result is attested by the
//! `arcium-gateway`'s `verify_output` callback (cluster attestation). There are no per-node Ed25519
//! signatures for our native committee check to consume, so settlement is authorized by a **registered
//! settlement authority** (`market.settlement_authority`) — the relayer/gateway key set at market init.
//! A transaction signed by that key may settle a batch with no committee signatures.
//!
//! The trust assumption is explicit: you trust the settlement authority to faithfully translate the
//! MXE's revealed clearing into the frame (the off-chain `mpc-core::relayer` does this deterministically
//! and cross-checks it against the gateway's revealed aggregates). The settlement *math* is identical
//! to the committee path — it shares [`crate::instructions::settle::apply_settlement`], which still
//! re-derives all economics from the fills and never trusts the header. A stronger future variant has
//! the gateway CPI directly into this instruction.
//!
//! Accounts:
//! 0. `[signer]`   settlement authority (== market.settlement_authority)
//! 1. `[writable]` market_state PDA
//! 2.. `[writable]` one `UserPosition` PDA per fill, in fill order

use {
    crate::{
        error::ShadowError,
        instructions::settle::apply_settlement,
        state::{cast, require_owned_by_program, MarketState},
        utils::{require_signer, require_writable},
    },
    pinocchio::{account::AccountView, error::ProgramError, ProgramResult},
};

pub fn process(accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    let [authority, market_ai, position_accounts @ ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    require_signer(authority)?;
    require_writable(market_ai)?;
    require_owned_by_program(market_ai)?;

    // Authorize: the market must have a settlement authority configured, and the signer must be it.
    {
        let md = market_ai.try_borrow()?;
        let m: &MarketState = cast(&md)?;
        if !m.trusted_settlement_enabled() || &m.settlement_authority != authority.address().as_array() {
            return Err(ShadowError::UnauthorizedSettlement.into());
        }
    }

    // Shared deterministic two-tier settlement (identical to the committee path).
    apply_settlement(market_ai, position_accounts, data)
}
