//! # ShadowState settlement engine (pure Pinocchio, `no_std` on-chain)
//!
//! A confidential prediction-market dark pool. Orders are privately matched off-chain by an Arcium
//! MPC committee; this program verifies the committee's signatures *natively* (Ed25519 precompile
//! + Instructions-sysvar introspection — no Anchor, no `solana-program`), runs deterministic
//! two-tier clearing, mutates a zero-copy `bytemuck::Pod` position ledger, and settles collateral
//! through Token-2022 `TransferChecked` CPIs.
//!
//! `no_std` is enabled only on the Solana target so the host build (`cargo test`) keeps `std` for
//! the test harness while the deployed SBF artifact remains heap-free (`no_allocator!`).

#![cfg_attr(target_os = "solana", no_std)]

use pinocchio::{account::AccountView, address::Address, error::ProgramError, ProgramResult};

pub mod error;
pub mod instructions;
pub mod math;
pub mod sig;
pub mod state;
pub mod utils;

/// Program ID. The deployed SBF program and the LiteSVM test harness both load at this address, so
/// PDA derivations (`find_program_address(.., &ID)`) and account-ownership checks line up.
///
/// `FP8riDGv8jrif5G8QfprfzPeNR8kQ3UUrpTp6EXByDVZ` — the deploy keypair at
/// `target/deploy/shadowstate_program-keypair.json`. On a real cluster the program ID **must** equal
/// the deployed address, because `invoke_signed` derives vault/PDA signers from the program's actual
/// address; the earlier `b"ShadowState1…"` vanity bytes were a local-only placeholder that no keypair
/// can ever own, so they could not be deployed.
pub const ID: Address = Address::new_from_array([
    213, 175, 71, 95, 34, 237, 217, 1, 101, 214, 62, 173, 183, 112, 167, 110, 147, 99, 114, 123, 15,
    94, 188, 99, 5, 77, 206, 93, 221, 252, 245, 8,
]);

// On-chain entrypoint + heap-free runtime. Compiled only for the Solana target; on the host the
// crate is plain `std` and these are omitted (the cdylib has no entrypoint, which is fine — tests
// load the SBF artifact, and the lib is consumed as an rlib).
#[cfg(target_os = "solana")]
mod bpf_entrypoint {
    pinocchio::program_entrypoint!(crate::process_instruction);
    pinocchio::no_allocator!();
    pinocchio::nostd_panic_handler!();
}

/// Instruction dispatch: 1-byte discriminator selects the handler (see `protocol::ids::ix`).
#[inline]
pub fn process_instruction(
    _program_id: &Address,
    accounts: &mut [AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    use protocol::ids::ix;

    let (disc, data) = instruction_data
        .split_first()
        .ok_or(ProgramError::InvalidInstructionData)?;

    match *disc {
        ix::INITIALIZE_MARKET => instructions::initialize_market::process(accounts, data),
        ix::INIT_USER_POSITION => instructions::init_user_position::process(accounts, data),
        ix::DEPOSIT_COLLATERAL => instructions::deposit_collateral::process(accounts, data),
        ix::SUBMIT_BATCH => instructions::submit_batch::process(accounts, data),
        ix::UPDATE_RISK_PARAMS => instructions::update_risk_params::process(accounts, data),
        ix::WITHDRAW_COLLATERAL => instructions::withdraw_collateral::process(accounts, data),
        ix::DEPOSIT_MM_COLLATERAL => instructions::deposit_mm_collateral::process(accounts, data),
        ix::RESOLVE_MARKET => instructions::resolve_market::process(accounts, data),
        ix::CLAIM_WINNINGS => instructions::claim_winnings::process(accounts, data),
        ix::CLAIM_MM_WINNINGS => instructions::claim_mm_winnings::process(accounts, data),
        ix::SUBMIT_BATCH_TRUSTED => instructions::submit_batch_trusted::process(accounts, data),
        ix::CLOSE_MARKET => instructions::close_market::process(accounts, data),
        ix::WITHDRAW_MM_COLLATERAL => instructions::withdraw_mm_collateral::process(accounts, data),
        _ => Err(ProgramError::InvalidInstructionData),
    }
}
