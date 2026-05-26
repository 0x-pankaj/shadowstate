//! Native multi-party signature verification — the on-chain replacement for Arcium's Anchor-only
//! `verify_output`. The "No Anchor" law is absolute, so the engine authenticates the FBA batch
//! itself using Solana's **Ed25519 precompile** plus **Instructions-sysvar introspection**.
//!
//! How it works: the relayer includes one or more Ed25519 precompile instructions in the same
//! transaction as `SubmitBatch`, each signed by an MPC committee member over the exact frame
//! bytes. The runtime cryptographically verifies those signatures *before* our program runs — so
//! if we are executing, every signature is already valid. This module's only job is therefore to
//! authorize **which** committee keys signed **which** message: it reads each Ed25519 instruction,
//! extracts the `(pubkey, message)` pairs, and confirms that at least `Committee.threshold`
//! distinct committee members signed our frame.
//!
//! Ed25519 precompile instruction-data layout (little-endian), as produced by
//! `solana_sdk::ed25519_instruction` and our test/relayer builder:
//! ```text
//!   [0]      num_signatures : u8
//!   [1]      padding        : u8
//!   per signature (14 bytes, starting at byte 2):
//!     signature_offset : u16,  signature_instruction_index : u16
//!     pubkey_offset    : u16,  pubkey_instruction_index    : u16
//!     message_offset   : u16,  message_size                : u16,  message_instruction_index : u16
//! ```
//! `*_instruction_index == u16::MAX` means "data lives in this same instruction" — the only form
//! we accept (a cross-instruction reference is rejected, so a signer can't be tricked into
//! authenticating bytes the program never inspects).

use {
    crate::{error::ShadowError, state::Committee},
    pinocchio::{
        account::AccountView, address::Address, error::ProgramError,
        sysvars::instructions::Instructions,
    },
};

/// Address of the Solana Ed25519 signature-verification precompile.
const ED25519_PROGRAM_ID: Address =
    Address::from_str_const("Ed25519SigVerify111111111111111111111111111");

const OFFSETS_START: usize = 2;
const OFFSETS_SIZE: usize = 14;
const SELF_INSTRUCTION: u16 = u16::MAX;

#[inline]
fn read_u16(data: &[u8], off: usize) -> Option<u16> {
    let b = data.get(off..off + 2)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

/// Verify that at least `committee.threshold` distinct committee members signed `message` via
/// Ed25519 precompile instructions in the current transaction.
///
/// `instructions_sysvar` must be the Instructions sysvar account
/// (`Sysvar1nstructions1111111111111111111111111`); `Instructions::try_from` enforces that.
pub fn verify_committee(
    instructions_sysvar: &AccountView,
    committee: &Committee,
    message: &[u8],
) -> Result<(), ProgramError> {
    let ixs = Instructions::try_from(instructions_sysvar)?;

    // One bit per committee member index — dedups multiple signatures from the same key.
    let mut signer_mask: u32 = 0;
    let mut saw_ed25519 = false;

    let num = ixs.num_instructions();
    for i in 0..num {
        let ix = ixs.load_instruction_at(i)?;
        if ix.get_program_id() != &ED25519_PROGRAM_ID {
            continue;
        }
        saw_ed25519 = true;
        let data = ix.get_instruction_data();

        let num_sigs = match data.first() {
            Some(&n) => n as usize,
            None => continue,
        };

        for s in 0..num_sigs {
            let base = OFFSETS_START + s * OFFSETS_SIZE;
            // Parse the six offsets we need (signature offset is verified by the runtime, ignored).
            let (Some(sig_ix), Some(pk_off), Some(pk_ix), Some(msg_off), Some(msg_size), Some(msg_ix)) = (
                read_u16(data, base + 2),
                read_u16(data, base + 4),
                read_u16(data, base + 6),
                read_u16(data, base + 8),
                read_u16(data, base + 10),
                read_u16(data, base + 12),
            ) else {
                continue;
            };

            // Only accept self-contained records (pubkey + message in this instruction's data).
            if sig_ix != SELF_INSTRUCTION || pk_ix != SELF_INSTRUCTION || msg_ix != SELF_INSTRUCTION
            {
                continue;
            }

            let pk_off = pk_off as usize;
            let msg_off = msg_off as usize;
            let msg_size = msg_size as usize;

            let Some(pk_bytes) = data.get(pk_off..pk_off + 32) else {
                continue;
            };
            let Some(signed_msg) = data.get(msg_off..msg_off + msg_size) else {
                continue;
            };

            // Bind the signature to *our* frame and to a known committee key.
            if signed_msg != message {
                continue;
            }
            let mut pk = [0u8; 32];
            pk.copy_from_slice(pk_bytes);
            if let Some(idx) = committee.index_of(&pk) {
                signer_mask |= 1u32 << idx;
            }
        }
    }

    if !saw_ed25519 {
        return Err(ShadowError::MissingSignatureProgram.into());
    }
    if signer_mask.count_ones() >= committee.threshold as u32 {
        Ok(())
    } else {
        Err(ShadowError::InsufficientCommitteeSignatures.into())
    }
}
