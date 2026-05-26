//! Small validation + account-creation helpers shared across instruction handlers. Manual
//! validation is mandatory under Pinocchio (no Anchor constraints) — these keep each handler's
//! checks terse and consistent.

use {
    crate::error::ShadowError,
    pinocchio::{
        account::AccountView,
        address::Address,
        cpi::{Seed, Signer},
        error::ProgramError,
        ProgramResult,
    },
    pinocchio_system::instructions::CreateAccount,
};

/// Require that `a` signed the transaction.
#[inline]
pub fn require_signer(a: &AccountView) -> Result<(), ProgramError> {
    if a.is_signer() {
        Ok(())
    } else {
        Err(ShadowError::MissingSignature.into())
    }
}

/// Require that `a` is marked writable.
#[inline]
pub fn require_writable(a: &AccountView) -> Result<(), ProgramError> {
    if a.is_writable() {
        Ok(())
    } else {
        Err(ShadowError::InvalidAccount.into())
    }
}

/// Require that `a`'s address equals `expected`.
#[inline]
pub fn require_address(a: &AccountView, expected: &Address) -> Result<(), ProgramError> {
    if a.address() == expected {
        Ok(())
    } else {
        Err(ShadowError::InvalidAccount.into())
    }
}

/// Validate that `account` is the canonical PDA for `seeds` (with stored `bump`) under this
/// program, without running the bump-search loop.
#[inline]
pub fn require_pda(
    account: &AccountView,
    seeds: &[&[u8]],
    bump: u8,
    program_id: &Address,
) -> Result<(), ProgramError> {
    // Rebuild the seed list with the bump appended.
    let bump_arr = [bump];
    // Up to 4 base seeds + bump in this program; stack array avoids allocation.
    let mut full: [&[u8]; 5] = [&[], &[], &[], &[], &[]];
    let n = seeds.len();
    if n > 4 {
        return Err(ShadowError::InvalidPda.into());
    }
    full[..n].copy_from_slice(seeds);
    full[n] = &bump_arr;

    let expected = Address::create_program_address(&full[..n + 1], program_id)
        .map_err(|_| ShadowError::InvalidPda)?;
    if account.address() != &expected {
        return Err(ShadowError::InvalidPda.into());
    }
    Ok(())
}

/// Create and allocate a program-owned PDA account, rent-funded by `payer` and signed by the
/// PDA's `seeds` (which must already include the bump as the final seed).
#[inline]
pub fn create_pda_account(
    payer: &AccountView,
    new_account: &AccountView,
    space: usize,
    signer_seeds: &[Seed],
    program_id: &Address,
) -> ProgramResult {
    CreateAccount::with_minimum_balance(payer, new_account, space as u64, program_id, None)?
        .invoke_signed(&[Signer::from(signer_seeds)])
}
