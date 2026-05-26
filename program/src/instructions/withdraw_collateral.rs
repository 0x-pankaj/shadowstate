//! `WithdrawCollateral` (disc 5): user withdraws free collateral. `TransferChecked` moves
//! Token-2022 collateral from the vault back to the user, signed by the vault PDA, and debits
//! `position.collateral`.
//!
//! Accounts:
//! 0. `[signer]`          owner (user)
//! 1. `[]`                market_state PDA
//! 2. `[writable]`        position PDA
//! 3. `[writable]`        vault token account (source; == market.vault)
//! 4. `[writable]`        user token account (dest)
//! 5. `[]`                collateral mint (== market.collateral_mint)
//! 6. `[]`                vault authority PDA `[b"vault", market]`
//! 7. `[]`                Token-2022 program

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

    let [owner, market_ai, position_ai, vault, user_token, mint_ai, vault_authority, token_program, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    require_signer(owner)?;
    require_writable(position_ai)?;
    require_writable(vault)?;
    require_writable(user_token)?;
    require_address(token_program, &pinocchio_token_2022::ID)?;

    let market_key = *market_ai.address().as_array();
    let owner_key = *owner.address().as_array();

    require_owned_by_program(market_ai)?;
    let (decimals, vault_bump) = {
        let md = market_ai.try_borrow()?;
        let m: &MarketState = cast(&md)?;
        require_address(vault, &Address::new_from_array(m.vault))?;
        require_address(mint_ai, &Address::new_from_array(m.collateral_mint))?;
        require_pda(vault_authority, &[seeds::VAULT, &market_key], m.vault_bump, &crate::ID)?;
        let mint = Mint::from_account_view(mint_ai)?;
        (mint.decimals(), m.vault_bump)
    };

    // Validate position + debit (must have sufficient free collateral).
    require_owned_by_program(position_ai)?;
    {
        let pd = position_ai.try_borrow()?;
        let pos: &UserPosition = cast(&pd)?;
        if pos.owner != owner_key || pos.market != market_key {
            return Err(ShadowError::InvalidAccount.into());
        }
        require_pda(position_ai, &[seeds::POSITION, &market_key, &owner_key], pos.bump, &crate::ID)?;
        if pos.collateral < amount {
            return Err(ShadowError::InsufficientCollateral.into());
        }
    }

    // Move funds vault -> user, signed by the vault PDA.
    TransferChecked {
        from: vault,
        mint: mint_ai,
        to: user_token,
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

    // Debit the ledger.
    let mut pd = position_ai.try_borrow_mut()?;
    let pos: &mut UserPosition = cast_mut(&mut pd)?;
    pos.collateral = math::sub(pos.collateral, amount)?;

    Ok(())
}
