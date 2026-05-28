//! The parameter modification portal: build + sign the on-chain `UpdateRiskParams` transaction.
//!
//! This is the authorized MM wallet's command pipeline into the Pinocchio contract. The instruction
//! data and account layout are the exact mirror of the program's `update_risk_params` handler:
//!
//! ```text
//!   data:     [disc=4] | base_oracle_price:u64 | max_skew_premium:u64 | imbalance_threshold:u64
//!   accounts: 0. [signer]   authority (== market.authority)
//!             1. [writable] market_state PDA
//! ```

use {
    crate::{error::Result, risk::RiskParams},
    protocol::ids::{ix, seeds},
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::{AccountMeta, Instruction},
    solana_keypair::Keypair,
    solana_signer::Signer,
    solana_transaction::Transaction,
};

/// The deployed ShadowState program ID (matches `shadowstate_program::ID`).
pub const SHADOWSTATE_PROGRAM_ID: Address = Address::new_from_array([
    0x53, 0x68, 0x61, 0x64, 0x6f, 0x77, 0x53, 0x74, 0x61, 0x74, 0x65, 0x31, 0x31, 0x31, 0x31, 0x31,
    0x31, 0x31, 0x31, 0x31, 0x31, 0x31, 0x31, 0x31, 0x31, 0x31, 0x31, 0x31, 0x31, 0x31, 0x31, 0x31,
]);

/// Derive the `MarketState` PDA for an authority: `[b"market", authority]`.
pub fn market_pda(authority: &Address, program_id: &Address) -> Address {
    Address::find_program_address(&[seeds::MARKET, authority.as_array()], program_id).0
}

/// Build the `UpdateRiskParams` instruction. Validates the params against the on-chain bounds first,
/// so a transaction is never assembled that the program would reject.
pub fn update_risk_ix(authority: &Address, market: &Address, params: &RiskParams) -> Result<Instruction> {
    params.validate()?;

    let mut data = Vec::with_capacity(1 + 24);
    data.push(ix::UPDATE_RISK_PARAMS);
    data.extend_from_slice(&params.base_oracle_price.to_le_bytes());
    data.extend_from_slice(&params.max_skew_premium.to_le_bytes());
    data.extend_from_slice(&params.imbalance_threshold.to_le_bytes());

    Ok(Instruction::new_with_bytes(
        SHADOWSTATE_PROGRAM_ID,
        &data,
        vec![
            AccountMeta::new_readonly(*authority, true),
            AccountMeta::new(*market, false),
        ],
    ))
}

/// Build a signed `UpdateRiskParams` transaction. The `authority` keypair is the MM admin wallet and
/// the sole signer + fee payer. `market` is its `MarketState` PDA (see [`market_pda`]).
pub fn update_risk_tx(
    authority: &Keypair,
    market: &Address,
    params: &RiskParams,
    recent_blockhash: Hash,
) -> Result<Transaction> {
    let ix = update_risk_ix(&authority.pubkey(), market, params)?;
    Ok(Transaction::new_signed_with_payer(
        &[ix],
        Some(&authority.pubkey()),
        &[authority],
        recent_blockhash,
    ))
}

#[cfg(test)]
mod tests {
    use {super::*, protocol::MIN_PRICE};

    fn good_params() -> RiskParams {
        RiskParams {
            base_oracle_price: 600_000,
            max_skew_premium: 50_000,
            imbalance_threshold: 500,
        }
    }

    #[test]
    fn ix_has_exact_on_chain_layout() {
        let authority = Address::new_from_array([1u8; 32]);
        let market = Address::new_from_array([2u8; 32]);
        let ix = update_risk_ix(&authority, &market, &good_params()).unwrap();

        assert_eq!(ix.program_id, SHADOWSTATE_PROGRAM_ID);
        assert_eq!(ix.data.len(), 1 + 24);
        assert_eq!(ix.data[0], protocol::ids::ix::UPDATE_RISK_PARAMS);
        assert_eq!(u64::from_le_bytes(ix.data[1..9].try_into().unwrap()), 600_000);
        assert_eq!(u64::from_le_bytes(ix.data[9..17].try_into().unwrap()), 50_000);
        assert_eq!(u64::from_le_bytes(ix.data[17..25].try_into().unwrap()), 500);

        assert_eq!(ix.accounts.len(), 2);
        assert!(ix.accounts[0].is_signer && !ix.accounts[0].is_writable, "authority signs, read-only");
        assert!(ix.accounts[1].is_writable && !ix.accounts[1].is_signer, "market writable");
        assert_eq!(ix.accounts[0].pubkey, authority);
        assert_eq!(ix.accounts[1].pubkey, market);
    }

    #[test]
    fn ix_rejects_out_of_bounds_params() {
        let authority = Address::new_from_array([1u8; 32]);
        let market = Address::new_from_array([2u8; 32]);
        let bad = RiskParams {
            base_oracle_price: MIN_PRICE - 1,
            max_skew_premium: 0,
            imbalance_threshold: 1,
        };
        assert!(update_risk_ix(&authority, &market, &bad).is_err());
    }

    #[test]
    fn tx_is_signed_by_authority() {
        let authority = Keypair::new();
        let market = market_pda(&authority.pubkey(), &SHADOWSTATE_PROGRAM_ID);
        let tx = update_risk_tx(&authority, &market, &good_params(), Hash::new_from_array([3u8; 32])).unwrap();
        assert!(tx.is_signed());
        assert_eq!(tx.signatures.len(), 1);
        assert_eq!(tx.message.account_keys[0], authority.pubkey());
    }

    #[test]
    fn market_pda_is_deterministic() {
        let authority = Address::new_from_array([9u8; 32]);
        let a = market_pda(&authority, &SHADOWSTATE_PROGRAM_ID);
        let b = market_pda(&authority, &SHADOWSTATE_PROGRAM_ID);
        assert_eq!(a, b);
    }
}
