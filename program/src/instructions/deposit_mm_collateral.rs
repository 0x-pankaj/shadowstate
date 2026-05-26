//! `DepositMmCollateral` (disc 6): fund the market-maker backstop. `TransferChecked` moves
//! Token-2022 collateral from a funder's token account into the vault and credits
//! `market.mm_collateral`. Anyone may fund the backstop (it only improves solvency); the tokens
//! become MM-reserved as Tier-2 residuals are settled in `SubmitBatch`.
//!
//! Accounts:
//! 0. `[signer]`   funder (token authority of the source account)
//! 1. `[writable]` market_state PDA
//! 2. `[writable]` funder token account (source)
//! 3. `[writable]` vault token account (dest; == market.vault)
//! 4. `[]`         collateral mint (== market.collateral_mint)
//! 5. `[]`         Token-2022 program

use {
    crate::{
        error::ShadowError,
        math,
        state::{cast, cast_mut, require_owned_by_program, MarketState},
        utils::{require_address, require_signer, require_writable},
    },
    pinocchio::{account::AccountView, address::Address, error::ProgramError, ProgramResult},
    pinocchio_token_2022::{instructions::TransferChecked, state::Mint},
};

pub fn process(accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    if data.len() < 8 {
        return Err(ShadowError::InvalidInstructionData.into());
    }
    let amount = u64::from_le_bytes(data[0..8].try_into().unwrap());
    if amount == 0 {
        return Err(ShadowError::InvalidInstructionData.into());
    }

    let [funder, market_ai, funder_token, vault, mint_ai, token_program, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    require_signer(funder)?;
    require_writable(market_ai)?;
    require_writable(funder_token)?;
    require_writable(vault)?;
    require_address(token_program, &pinocchio_token_2022::ID)?;
    require_owned_by_program(market_ai)?;

    let decimals = {
        let md = market_ai.try_borrow()?;
        let m: &MarketState = cast(&md)?;
        require_address(vault, &Address::new_from_array(m.vault))?;
        require_address(mint_ai, &Address::new_from_array(m.collateral_mint))?;
        Mint::from_account_view(mint_ai)?.decimals()
    };

    // Move funds: funder -> vault, authorized by the funder.
    TransferChecked {
        from: funder_token,
        mint: mint_ai,
        to: vault,
        authority: funder,
        amount,
        decimals,
        token_program: &pinocchio_token_2022::ID,
    }
    .invoke()?;

    // Credit the MM backstop pool.
    let mut md = market_ai.try_borrow_mut()?;
    let m: &mut MarketState = cast_mut(&mut md)?;
    m.mm_collateral = math::add(m.mm_collateral, amount)?;

    Ok(())
}
