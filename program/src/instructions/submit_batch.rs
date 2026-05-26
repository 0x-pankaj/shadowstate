//! `SubmitBatch` (disc 3): the **committee-verified** Frequent-Batch-Auction settlement. The relayer
//! submits a committee-signed batch frame; the engine authenticates it natively (Ed25519 precompile +
//! Instructions sysvar), then runs the shared deterministic two-tier settlement
//! ([`crate::instructions::settle::apply_settlement`]).
//!
//! This is the **self-operated-committee** trust path. The Arcium path uses
//! [`crate::instructions::submit_batch_trusted`] instead, where a registered settlement authority
//! signs and the trust anchor is the MXE attestation. Both share the identical settlement core, so
//! the economics and full-collateralization guarantees are the same; only the *authentication*
//! differs.
//!
//! Settlement moves no tokens — it is a pure ledger update; full collateralization is maintained by
//! the MM backstop reservation (see `settle.rs`). The engine never trusts the header's economics — it
//! re-derives them from the fills and requires a threshold of committee signatures over the exact
//! frame bytes.
//!
//! Accounts:
//! 0. `[signer]`   relayer
//! 1. `[writable]` market_state PDA
//! 2. `[]`         committee PDA `[b"committee", market]`
//! 3. `[]`         instructions sysvar
//! 4. `[writable]` vault token account (== market.vault)
//! 5. `[writable]` MM account (== market.mm_account)
//! 6. `[]`         collateral mint (== market.collateral_mint)
//! 7. `[]`         vault authority PDA `[b"vault", market]`
//! 8. `[]`         Token-2022 program
//! 9.. `[writable]` one `UserPosition` PDA per fill, in fill order

use {
    crate::{
        instructions::settle::apply_settlement,
        sig::verify_committee,
        state::{cast, require_owned_by_program, Committee, MarketState},
        utils::{require_address, require_pda, require_signer, require_writable},
    },
    pinocchio::{account::AccountView, address::Address, error::ProgramError, ProgramResult},
    protocol::ids::seeds,
};

pub fn process(accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    let [relayer, market_ai, committee_ai, ix_sysvar, vault, mm_ai, mint_ai, vault_authority, token_program, position_accounts @ ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    require_signer(relayer)?;
    require_writable(market_ai)?;
    require_writable(vault)?;
    require_writable(mm_ai)?;
    require_address(token_program, &pinocchio_token_2022::ID)?;
    require_owned_by_program(market_ai)?;

    let market_key = *market_ai.address().as_array();

    // Validate the market's economic accounts (stable settlement account list; no token movement).
    {
        let md = market_ai.try_borrow()?;
        let m: &MarketState = cast(&md)?;
        require_address(vault, &Address::new_from_array(m.vault))?;
        require_address(mm_ai, &Address::new_from_array(m.mm_account))?;
        require_address(mint_ai, &Address::new_from_array(m.collateral_mint))?;
        require_pda(vault_authority, &[seeds::VAULT, &market_key], m.vault_bump, &crate::ID)?;
    }

    // Native committee verification over the exact frame bytes.
    require_owned_by_program(committee_ai)?;
    {
        let (committee_pda, _) =
            Address::find_program_address(&[seeds::COMMITTEE, &market_key], &crate::ID);
        require_address(committee_ai, &committee_pda)?;
        let cd = committee_ai.try_borrow()?;
        let committee: &Committee = cast(&cd)?;
        verify_committee(ix_sysvar, committee, data)?;
    }

    // Shared deterministic two-tier settlement.
    apply_settlement(market_ai, position_accounts, data)
}
