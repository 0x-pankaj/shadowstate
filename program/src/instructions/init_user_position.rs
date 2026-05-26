//! `InitUserPosition` (disc 1): create a user's `UserPosition` ledger PDA for a market.
//!
//! Accounts:
//! 0. `[signer, writable]` payer
//! 1. `[signer]`          owner (the user this position belongs to)
//! 2. `[]`                market_state PDA
//! 3. `[writable]`        position PDA `[b"pos", market, owner]`
//! 4. `[]`                system program

use {
    crate::{
        state::{
            cast, cast_uninit_mut, require_owned_by_program, AccountState, MarketState, UserPosition,
        },
        utils::{create_pda_account, require_address, require_signer, require_writable},
    },
    pinocchio::{account::AccountView, address::Address, cpi::Seed, error::ProgramError, ProgramResult},
    protocol::ids::{account, seeds, ACCOUNT_VERSION},
};

pub fn process(accounts: &mut [AccountView], _data: &[u8]) -> ProgramResult {
    let [payer, owner, market_ai, position_ai, _system, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    require_signer(payer)?;
    require_signer(owner)?;
    require_writable(position_ai)?;

    // Validate the market account belongs to this program and is a MarketState.
    require_owned_by_program(market_ai)?;
    {
        let md = market_ai.try_borrow()?;
        let _m: &MarketState = cast(&md)?;
    }

    let program_id = &crate::ID;
    let market_key = *market_ai.address().as_array();
    let owner_key = *owner.address().as_array();

    let (position_pda, bump) =
        Address::find_program_address(&[seeds::POSITION, &market_key, &owner_key], program_id);
    require_address(position_ai, &position_pda)?;

    create_pda_account(
        payer,
        position_ai,
        UserPosition::LEN,
        &[
            Seed::from(seeds::POSITION),
            Seed::from(&market_key),
            Seed::from(&owner_key),
            Seed::from(&[bump]),
        ],
        program_id,
    )?;

    let mut pd = position_ai.try_borrow_mut()?;
    let pos: &mut UserPosition = cast_uninit_mut(&mut pd)?;
    pos.disc = account::USER_POSITION;
    pos.version = ACCOUNT_VERSION;
    pos.bump = bump;
    pos.owner = owner_key;
    pos.market = market_key;
    pos.yes_qty = 0;
    pos.no_qty = 0;
    pos.collateral = 0;

    Ok(())
}
