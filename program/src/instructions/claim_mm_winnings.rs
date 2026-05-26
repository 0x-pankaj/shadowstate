//! `ClaimMmWinnings` (disc 9): after resolution, the market-maker redeems its winning backstop
//! contracts for `$1` each. `TransferChecked` moves the payout from the vault to the MM token account
//! (signed by the vault PDA) and zeroes both MM legs. Symmetric to `ClaimWinnings` but for the MM's
//! `mm_yes` / `mm_no` aggregate positions.
//!
//! Accounts:
//! 0. `[signer]`   authority (== market.authority)
//! 1. `[writable]` market_state PDA
//! 2. `[writable]` MM token account (dest; == market.mm_account)
//! 3. `[writable]` vault token account (source; == market.vault)
//! 4. `[]`         collateral mint (== market.collateral_mint)
//! 5. `[]`         vault authority PDA `[b"vault", market]`
//! 6. `[]`         Token-2022 program

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
    protocol::{ids::seeds, MIDPOINT_PRICE, OUTCOME_INVALID, OUTCOME_NO_WON, OUTCOME_YES_WON, SCALE_FACTOR},
};

pub fn process(accounts: &mut [AccountView], _data: &[u8]) -> ProgramResult {
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

    // Validate authority + bind accounts; compute payout on the MM's winning leg.
    let (decimals, vault_bump, payout) = {
        let md = market_ai.try_borrow()?;
        let m: &MarketState = cast(&md)?;
        if &m.authority != authority.address().as_array() {
            return Err(ShadowError::MissingSignature.into());
        }
        require_address(mm_token, &Address::new_from_array(m.mm_account))?;
        require_address(vault, &Address::new_from_array(m.vault))?;
        require_address(mint_ai, &Address::new_from_array(m.collateral_mint))?;
        require_pda(vault_authority, &[seeds::VAULT, &market_key], m.vault_bump, &crate::ID)?;

        let payout = match m.outcome {
            OUTCOME_YES_WON => math::collateral_for(m.mm_yes, SCALE_FACTOR)?,
            OUTCOME_NO_WON => math::collateral_for(m.mm_no, SCALE_FACTOR)?,
            // Voided market: both MM legs settle at the $0.50 midpoint.
            OUTCOME_INVALID => math::add(
                math::collateral_for(m.mm_yes, MIDPOINT_PRICE)?,
                math::collateral_for(m.mm_no, MIDPOINT_PRICE)?,
            )?,
            _ => return Err(ShadowError::MarketNotResolved.into()),
        };
        (Mint::from_account_view(mint_ai)?.decimals(), m.vault_bump, payout)
    };
    if payout == 0 {
        return Err(ShadowError::NothingToClaim.into());
    }

    TransferChecked {
        from: vault,
        mint: mint_ai,
        to: mm_token,
        authority: vault_authority,
        amount: payout,
        decimals,
        token_program: &pinocchio_token_2022::ID,
    }
    .invoke_signed(&[Signer::from(&[
        Seed::from(seeds::VAULT),
        Seed::from(&market_key),
        Seed::from(&[vault_bump]),
    ])])?;

    // Zero both MM legs so the backstop cannot be claimed twice.
    let mut md = market_ai.try_borrow_mut()?;
    let m: &mut MarketState = cast_mut(&mut md)?;
    m.mm_yes = 0;
    m.mm_no = 0;

    Ok(())
}
