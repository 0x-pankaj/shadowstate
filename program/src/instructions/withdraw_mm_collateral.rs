//! `WithdrawMmCollateral` (disc 12): the MM reclaims its **unreserved** backstop float. `mm_collateral`
//! already excludes amounts reserved by settled batches (each Tier-2 residual subtracts its
//! obligation at settlement), so the full `mm_collateral` balance is free to withdraw — the funds
//! backing live contracts are locked in the vault outside this counter and cannot be touched here.
//! `TransferChecked` moves the amount vault → MM account, signed by the vault PDA.
//!
//! Accounts:
//! 0. `[signer]`   authority (== market.authority)
//! 1. `[writable]` market_state PDA
//! 2. `[writable]` MM token account (dest; == market.mm_account)
//! 3. `[writable]` vault token account (source; == market.vault)
//! 4. `[]`         collateral mint (== market.collateral_mint)
//! 5. `[]`         vault authority PDA `[b"vault", market]`
//! 6. `[]`         Token-2022 program
//!
//! Data: `amount: u64`.

use {
    crate::{
        error::ShadowError,
        math,
        state::{cast, cast_mut, require_owned_by_program, MarketState},
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
    protocol::ids::seeds,
};

pub fn process(accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    if data.len() < 8 {
        return Err(ShadowError::InvalidInstructionData.into());
    }
    let amount = u64::from_le_bytes(data[0..8].try_into().unwrap());
    if amount == 0 {
        return Err(ShadowError::InvalidInstructionData.into());
    }

    let [authority, market_ai, mm_token, vault, mint_ai, vault_authority, token_program, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    require_signer(authority)?;
    require_writable(market_ai)?;
    require_writable(mm_token)?;
    require_writable(vault)?;
    require_address(token_program, &pinocchio_token_2022::ID)?;
    require_owned_by_program(market_ai)?;

    let market_key = *market_ai.address().as_array();

    // Validate authority + bind accounts; check the unreserved float covers the request.
    let (decimals, vault_bump) = {
        let md = market_ai.try_borrow()?;
        let m: &MarketState = cast(&md)?;
        if &m.authority != authority.address().as_array() {
            return Err(ShadowError::MissingSignature.into());
        }
        require_address(mm_token, &Address::new_from_array(m.mm_account))?;
        require_address(vault, &Address::new_from_array(m.vault))?;
        require_address(mint_ai, &Address::new_from_array(m.collateral_mint))?;
        require_pda(vault_authority, &[seeds::VAULT, &market_key], m.vault_bump, &crate::ID)?;
        if m.mm_collateral < amount {
            return Err(ShadowError::InsufficientMmCollateral.into());
        }
        (Mint::from_account_view(mint_ai)?.decimals(), m.vault_bump)
    };

    TransferChecked {
        from: vault,
        mint: mint_ai,
        to: mm_token,
        authority: vault_authority,
        amount,
        decimals,
        token_program: &pinocchio_token_2022::ID,
    }
    .invoke_signed(&[Signer::from(&[
        Seed::from(seeds::VAULT),
        Seed::from(&market_key),
        Seed::from(&[vault_bump]),
    ])])?;

    // Debit the unreserved float.
    let mut md = market_ai.try_borrow_mut()?;
    let m: &mut MarketState = cast_mut(&mut md)?;
    m.mm_collateral = math::sub(m.mm_collateral, amount)?;

    Ok(())
}
