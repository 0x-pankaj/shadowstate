//! `DepositCollateral` (disc 2): user funds their position. `TransferChecked` moves Token-2022
//! collateral from the user's token account into the market vault and credits `position.collateral`.
//!
//! Accounts:
//! 0. `[signer]`          owner (user; token authority of the source account)
//! 1. `[]`                market_state PDA
//! 2. `[writable]`        position PDA `[b"pos", market, owner]`
//! 3. `[writable]`        user token account (source)
//! 4. `[writable]`        vault token account (dest; == market.vault)
//! 5. `[]`                collateral mint (== market.collateral_mint)
//! 6. `[]`                Token-2022 program

use {
    crate::{
        error::ShadowError,
        math,
        state::{cast, cast_mut, require_owned_by_program, MarketState, UserPosition},
        utils::{require_address, require_pda, require_signer, require_writable},
    },
    pinocchio::{account::AccountView, error::ProgramError, ProgramResult},
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

    let [owner, market_ai, position_ai, user_token, vault, mint_ai, token_program, ..] = accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    require_signer(owner)?;
    require_writable(position_ai)?;
    require_writable(user_token)?;
    require_writable(vault)?;
    require_address(token_program, &pinocchio_token_2022::ID)?;

    let market_key = *market_ai.address().as_array();
    let owner_key = *owner.address().as_array();

    // Validate market + bind vault/mint to it.
    require_owned_by_program(market_ai)?;
    let decimals = {
        let md = market_ai.try_borrow()?;
        let m: &MarketState = cast(&md)?;
        require_address(vault, &pinocchio::address::Address::new_from_array(m.vault))?;
        require_address(
            mint_ai,
            &pinocchio::address::Address::new_from_array(m.collateral_mint),
        )?;
        let mint = Mint::from_account_view(mint_ai)?;
        mint.decimals()
    };

    // Validate the position PDA + ownership.
    require_owned_by_program(position_ai)?;
    {
        let pd = position_ai.try_borrow()?;
        let pos: &UserPosition = cast(&pd)?;
        if pos.owner != owner_key || pos.market != market_key {
            return Err(ShadowError::InvalidAccount.into());
        }
        require_pda(
            position_ai,
            &[seeds::POSITION, &market_key, &owner_key],
            pos.bump,
            &crate::ID,
        )?;
    }

    // Move funds: user -> vault, authorized by the user (a plain signer).
    TransferChecked {
        from: user_token,
        mint: mint_ai,
        to: vault,
        authority: owner,
        amount,
        decimals,
        token_program: &pinocchio_token_2022::ID,
    }
    .invoke()?;

    // Credit the ledger.
    let mut pd = position_ai.try_borrow_mut()?;
    let pos: &mut UserPosition = cast_mut(&mut pd)?;
    pos.collateral = math::add(pos.collateral, amount)?;

    Ok(())
}
