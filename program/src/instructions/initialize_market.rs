//! `InitializeMarket` (disc 0): create the `MarketState` and immutable `Committee` PDAs, set risk
//! params, the collateral mint/vault, and the trusted MPC committee.
//!
//! Accounts:
//! 0. `[signer, writable]` payer (rent funder)
//! 1. `[signer]`          authority (MM admin; stored, gates `UpdateRiskParams`)
//! 2. `[]`                collateral mint (Token-2022)
//! 3. `[]`                vault token account (Token-2022; token authority == vault PDA)
//! 4. `[]`                MM token account (Token-2022; same mint; receives MM backstop winnings)
//! 5. `[writable]`        market_state PDA  `[b"market", authority]`
//! 6. `[writable]`        committee PDA     `[b"committee", market_state]`
//! 7. `[]`                system program
//!
//! Data: `base_oracle_price:u64 | max_skew_premium:u64 | imbalance_threshold:u64 | count:u8 |
//! threshold:u8 | members:[u8;32]*count`.

use {
    crate::{
        error::ShadowError,
        state::{cast_uninit_mut, AccountState, Committee, MarketState},
        utils::{create_pda_account, require_address, require_signer, require_writable},
    },
    pinocchio::{
        account::AccountView,
        address::Address,
        cpi::Seed,
        error::ProgramError,
        ProgramResult,
    },
    protocol::{
        ids::{account, seeds, ACCOUNT_VERSION},
        MAX_COMMITTEE, MAX_PRICE, MIN_PRICE,
    },
};

struct Params<'a> {
    base_oracle_price: u64,
    max_skew_premium: u64,
    imbalance_threshold: u64,
    count: u8,
    threshold: u8,
    members: &'a [u8],
    /// Optional trusted gateway settlement authority; all-zero ⇒ trusted path disabled.
    settlement_authority: [u8; 32],
}

fn parse(data: &[u8]) -> Result<Params<'_>, ProgramError> {
    // 8 + 8 + 8 + 1 + 1 = 26 byte fixed header.
    if data.len() < 26 {
        return Err(ShadowError::InvalidInstructionData.into());
    }
    let base_oracle_price = u64::from_le_bytes(data[0..8].try_into().unwrap());
    let max_skew_premium = u64::from_le_bytes(data[8..16].try_into().unwrap());
    let imbalance_threshold = u64::from_le_bytes(data[16..24].try_into().unwrap());
    let count = data[24];
    let threshold = data[25];

    let members_end = 26 + count as usize * 32;
    let members = data
        .get(26..members_end)
        .ok_or(ShadowError::InvalidInstructionData)?;
    // Optional trailing 32-byte settlement authority (the Arcium gateway path); absent ⇒ disabled.
    let settlement_authority = if data.len() == members_end {
        [0u8; 32]
    } else if data.len() == members_end + 32 {
        let mut sa = [0u8; 32];
        sa.copy_from_slice(&data[members_end..members_end + 32]);
        sa
    } else {
        return Err(ShadowError::InvalidInstructionData.into());
    };
    Ok(Params {
        base_oracle_price,
        max_skew_premium,
        imbalance_threshold,
        count,
        threshold,
        members,
        settlement_authority,
    })
}

fn validate(p: &Params) -> Result<(), ProgramError> {
    if p.count == 0 || p.count as usize > MAX_COMMITTEE {
        return Err(ShadowError::InvalidCommitteeConfig.into());
    }
    if p.threshold == 0 || p.threshold > p.count {
        return Err(ShadowError::InvalidCommitteeConfig.into());
    }
    if p.imbalance_threshold == 0 {
        return Err(ShadowError::InvalidRiskParams.into());
    }
    if p.base_oracle_price < MIN_PRICE || p.base_oracle_price > MAX_PRICE {
        return Err(ShadowError::InvalidRiskParams.into());
    }
    if p.max_skew_premium > MAX_PRICE {
        return Err(ShadowError::InvalidRiskParams.into());
    }
    Ok(())
}

pub fn process(accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    let p = parse(data)?;
    validate(&p)?;

    let [payer, authority, collateral_mint, vault, mm_account, market_ai, committee_ai, _system, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    require_signer(payer)?;
    require_signer(authority)?;
    require_writable(market_ai)?;
    require_writable(committee_ai)?;

    let program_id = &crate::ID;
    let authority_key = *authority.address().as_array();

    // Derive + validate the three PDAs (canonical bumps).
    let (market_pda, market_bump) =
        Address::find_program_address(&[seeds::MARKET, &authority_key], program_id);
    require_address(market_ai, &market_pda)?;
    let market_key = *market_pda.as_array();

    let (committee_pda, committee_bump) =
        Address::find_program_address(&[seeds::COMMITTEE, &market_key], program_id);
    require_address(committee_ai, &committee_pda)?;

    let (vault_pda, vault_bump) =
        Address::find_program_address(&[seeds::VAULT, &market_key], program_id);

    // Collateral mint must be a Token-2022 account; vault must be a Token-2022 token account whose
    // token authority is the vault PDA (so only this program can move settlement funds).
    if !collateral_mint.owned_by(&pinocchio_token_2022::ID) {
        return Err(ShadowError::InvalidAccount.into());
    }
    {
        let vault_token = pinocchio_token_2022::state::Account::from_account_view(vault)?;
        if vault_token.owner() != &vault_pda {
            return Err(ShadowError::InvalidAccount.into());
        }
        if vault_token.mint() != collateral_mint.address() {
            return Err(ShadowError::InvalidAccount.into());
        }
    }
    // MM fee account: a Token-2022 account on the same mint.
    {
        let mm_token = pinocchio_token_2022::state::Account::from_account_view(mm_account)?;
        if mm_token.mint() != collateral_mint.address() {
            return Err(ShadowError::InvalidAccount.into());
        }
    }

    // Create the market_state PDA, signed by its seeds.
    create_pda_account(
        payer,
        market_ai,
        MarketState::LEN,
        &[
            Seed::from(seeds::MARKET),
            Seed::from(&authority_key),
            Seed::from(&[market_bump]),
        ],
        program_id,
    )?;
    // Create the committee PDA.
    create_pda_account(
        payer,
        committee_ai,
        Committee::LEN,
        &[
            Seed::from(seeds::COMMITTEE),
            Seed::from(&market_key),
            Seed::from(&[committee_bump]),
        ],
        program_id,
    )?;

    // Write MarketState.
    {
        let mut md = market_ai.try_borrow_mut()?;
        let m: &mut MarketState = cast_uninit_mut(&mut md)?;
        m.disc = account::MARKET_STATE;
        m.version = ACCOUNT_VERSION;
        m.bump = market_bump;
        m.vault_bump = vault_bump;
        m.authority = authority_key;
        m.collateral_mint = *collateral_mint.address().as_array();
        m.vault = *vault.address().as_array();
        m.mm_account = *mm_account.address().as_array();
        m.base_oracle_price = p.base_oracle_price;
        m.max_skew_premium = p.max_skew_premium;
        m.imbalance_threshold = p.imbalance_threshold;
        m.total_yes_supply = 0;
        m.total_no_supply = 0;
        m.mm_yes = 0;
        m.mm_no = 0;
        m.last_epoch = 0;
        m.outcome = protocol::OUTCOME_UNRESOLVED;
        m.status = protocol::STATUS_TRADING;
        m.mm_collateral = 0;
        m.settlement_authority = p.settlement_authority;
    }

    // Write the immutable Committee.
    {
        let mut cd = committee_ai.try_borrow_mut()?;
        let c: &mut Committee = cast_uninit_mut(&mut cd)?;
        c.disc = account::COMMITTEE;
        c.version = ACCOUNT_VERSION;
        c.count = p.count;
        c.threshold = p.threshold;
        for i in 0..p.count as usize {
            c.members[i].copy_from_slice(&p.members[i * 32..i * 32 + 32]);
        }
    }

    Ok(())
}
