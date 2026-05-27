//! The on-chain push: turn a [`SignedBatch`] into the exact transaction the Pinocchio settlement
//! engine accepts — a sequence of Ed25519 precompile instructions (one per committee signature)
//! followed by the `SubmitBatch` instruction, with the `UserPosition` accounts in fill order.
//!
//! The Ed25519 instruction byte layout and the `SubmitBatch` account ordering here are the precise
//! mirror image of the on-chain `sig::verify_committee` parser and `submit_batch::process` account
//! destructuring; the on-chain LiteSVM suite builds identical bytes.

use {
    crate::{committee::NodeSignature, engine::SignedBatch, relayer::RelayedBatch},
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
/// Token-2022 program.
pub const TOKEN_2022_PROGRAM_ID: Address =
    Address::from_str_const("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
/// Ed25519 signature-verification precompile.
pub const ED25519_PROGRAM_ID: Address =
    Address::from_str_const("Ed25519SigVerify111111111111111111111111111");
/// Instructions sysvar (introspected on-chain for the committee signatures).
pub const INSTRUCTIONS_SYSVAR: Address =
    Address::from_str_const("Sysvar1nstructions1111111111111111111111111");

// Ed25519 precompile data layout (one self-contained signature).
const ED_PK_OFF: u16 = 16;
const ED_SIG_OFF: u16 = 48;
const ED_MSG_OFF: u16 = 112;
const SELF_IX: u16 = u16::MAX;

/// The addresses the relayer keeps for a market (learned from config / a one-time RPC read of the
/// `MarketState` account). The PDAs are derived deterministically and need not be stored.
#[derive(Debug, Clone, Copy)]
pub struct MarketConfig {
    /// Deployed program ID (defaults to [`SHADOWSTATE_PROGRAM_ID`] via [`MarketConfig::new`]).
    pub program_id: Address,
    /// The `MarketState` PDA.
    pub market: Address,
    /// Token-2022 vault token account (`MarketState.vault`).
    pub vault: Address,
    /// MM fee-recipient token account (`MarketState.mm_account`).
    pub mm_account: Address,
    /// Token-2022 collateral mint (`MarketState.collateral_mint`).
    pub mint: Address,
}

impl MarketConfig {
    /// Build a config against the canonical deployed program ID.
    pub fn new(market: Address, vault: Address, mm_account: Address, mint: Address) -> Self {
        Self {
            program_id: SHADOWSTATE_PROGRAM_ID,
            market,
            vault,
            mm_account,
            mint,
        }
    }

    /// Committee PDA: `[b"committee", market]`.
    pub fn committee_pda(&self) -> Address {
        Address::find_program_address(&[seeds::COMMITTEE, self.market.as_array()], &self.program_id).0
    }

    /// Vault authority PDA: `[b"vault", market]`.
    pub fn vault_authority_pda(&self) -> Address {
        Address::find_program_address(&[seeds::VAULT, self.market.as_array()], &self.program_id).0
    }

    /// Per-user position PDA: `[b"pos", market, owner]`.
    pub fn position_pda(&self, owner: &[u8; 32]) -> Address {
        Address::find_program_address(
            &[seeds::POSITION, self.market.as_array(), owner],
            &self.program_id,
        )
        .0
    }
}

/// Build one Ed25519 precompile instruction carrying a single committee signature over `message`.
/// Layout matches `program::sig`: header(16) | pubkey@16 | signature@48 | message@112.
pub fn ed25519_precompile_ix(sig: &NodeSignature, message: &[u8]) -> Instruction {
    let mut d = Vec::with_capacity(ED_MSG_OFF as usize + message.len());
    d.push(1); // num_signatures
    d.push(0); // padding
    d.extend_from_slice(&ED_SIG_OFF.to_le_bytes());
    d.extend_from_slice(&SELF_IX.to_le_bytes()); // signature instruction index = self
    d.extend_from_slice(&ED_PK_OFF.to_le_bytes());
    d.extend_from_slice(&SELF_IX.to_le_bytes()); // pubkey instruction index = self
    d.extend_from_slice(&ED_MSG_OFF.to_le_bytes());
    d.extend_from_slice(&(message.len() as u16).to_le_bytes());
    d.extend_from_slice(&SELF_IX.to_le_bytes()); // message instruction index = self
    d.extend_from_slice(&sig.pubkey); // @16
    d.extend_from_slice(&sig.signature); // @48
    d.extend_from_slice(message); // @112
    Instruction::new_with_bytes(ED25519_PROGRAM_ID, &d, vec![])
}

/// Build the `SubmitBatch` instruction (discriminator ++ frame) with the exact account ordering the
/// on-chain handler destructures, including one writable `UserPosition` per fill in fill order.
pub fn submit_batch_ix(
    config: &MarketConfig,
    relayer: &Address,
    frame: &[u8],
    positions: &[Address],
) -> Instruction {
    let mut data = Vec::with_capacity(1 + frame.len());
    data.push(ix::SUBMIT_BATCH);
    data.extend_from_slice(frame);

    let mut metas = vec![
        AccountMeta::new_readonly(*relayer, true),
        AccountMeta::new(config.market, false),
        AccountMeta::new_readonly(config.committee_pda(), false),
        AccountMeta::new_readonly(INSTRUCTIONS_SYSVAR, false),
        AccountMeta::new(config.vault, false),
        AccountMeta::new(config.mm_account, false),
        AccountMeta::new_readonly(config.mint, false),
        AccountMeta::new_readonly(config.vault_authority_pda(), false),
        AccountMeta::new_readonly(TOKEN_2022_PROGRAM_ID, false),
    ];
    for p in positions {
        metas.push(AccountMeta::new(*p, false));
    }
    Instruction::new_with_bytes(config.program_id, &data, metas)
}

/// Assemble the full instruction list for a signed batch: one Ed25519 instruction per signature,
/// then `SubmitBatch`. The `UserPosition` accounts are derived from the batch's fill order.
pub fn build_instructions(config: &MarketConfig, relayer: &Address, batch: &SignedBatch) -> Vec<Instruction> {
    let mut ixs: Vec<Instruction> = batch
        .signatures
        .iter()
        .map(|s| ed25519_precompile_ix(s, &batch.frame))
        .collect();
    let positions: Vec<Address> = batch
        .fill_owners()
        .iter()
        .map(|owner| config.position_pda(owner))
        .collect();
    ixs.push(submit_batch_ix(config, relayer, &batch.frame, &positions));
    ixs
}

/// Build a signed `SubmitBatch` transaction ready to broadcast. The `relayer` pays fees and is the
/// transaction's sole signer (the committee's authority rides in the Ed25519 instructions, verified
/// by the runtime precompile — no committee key signs the transaction itself).
pub fn build_transaction(
    config: &MarketConfig,
    relayer: &Keypair,
    batch: &SignedBatch,
    recent_blockhash: Hash,
) -> Transaction {
    let ixs = build_instructions(config, &relayer.pubkey(), batch);
    Transaction::new_signed_with_payer(&ixs, Some(&relayer.pubkey()), &[relayer], recent_blockhash)
}

// ---- trusted gateway path (`SubmitBatchTrusted`) ----------------------------------------------

/// Build the `SubmitBatchTrusted` instruction: discriminator ++ frame, with `[authority(signer),
/// market(writable), positions(writable)..]` — no committee, no Ed25519 instructions. The authority
/// must equal the market's registered `settlement_authority`.
pub fn submit_batch_trusted_ix(
    config: &MarketConfig,
    authority: &Address,
    frame: &[u8],
    positions: &[Address],
) -> Instruction {
    let mut data = Vec::with_capacity(1 + frame.len());
    data.push(ix::SUBMIT_BATCH_TRUSTED);
    data.extend_from_slice(frame);

    let mut metas = vec![
        AccountMeta::new_readonly(*authority, true),
        AccountMeta::new(config.market, false),
    ];
    for p in positions {
        metas.push(AccountMeta::new(*p, false));
    }
    Instruction::new_with_bytes(config.program_id, &data, metas)
}

/// Build a signed `SubmitBatchTrusted` transaction from a [`RelayedBatch`]. `authority` is the
/// registered settlement authority and the sole signer + fee payer — no committee signatures are
/// needed (the trust anchor is the off-chain Arcium MXE attestation).
pub fn build_trusted_transaction(
    config: &MarketConfig,
    authority: &Keypair,
    relayed: &RelayedBatch,
    recent_blockhash: Hash,
) -> Transaction {
    let positions: Vec<Address> = relayed
        .fill_owners()
        .iter()
        .map(|owner| config.position_pda(owner))
        .collect();
    let ix = submit_batch_trusted_ix(config, &authority.pubkey(), &relayed.frame, &positions);
    Transaction::new_signed_with_payer(&[ix], Some(&authority.pubkey()), &[authority], recent_blockhash)
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            committee::Committee,
            engine::{process_epoch, EpochParams},
            mxe::MxeCluster,
            order::{Order, Side},
            seal::{seal_order_with, ClusterKey},
        },
        x25519_dalek::StaticSecret,
    };

    // Re-implementation of the on-chain `sig.rs` offset parsing, to prove byte compatibility.
    fn parse_ed25519(data: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let read_u16 = |off: usize| u16::from_le_bytes([data[off], data[off + 1]]);
        assert_eq!(data[0], 1, "num_signatures");
        let base = 2;
        assert_eq!(read_u16(base + 2), SELF_IX); // sig ix index
        let pk_off = read_u16(base + 4) as usize;
        assert_eq!(read_u16(base + 6), SELF_IX); // pk ix index
        let msg_off = read_u16(base + 8) as usize;
        let msg_size = read_u16(base + 10) as usize;
        assert_eq!(read_u16(base + 12), SELF_IX); // msg ix index
        (
            data[pk_off..pk_off + 32].to_vec(),
            data[msg_off..msg_off + msg_size].to_vec(),
        )
    }

    fn make_batch() -> (Committee, crate::engine::SignedBatch) {
        let cluster = ClusterKey::from_seed([1u8; 32]);
        let mut mxe = MxeCluster::new(4, 0x99);
        let seeds: Vec<[u8; 32]> = (1..=3u8)
            .map(|i| {
                let mut s = [0u8; 32];
                s[0] = i;
                s
            })
            .collect();
        let committee = Committee::from_seeds(&seeds, 2).unwrap();
        let market = [7u8; 32];
        let seal = |user: u8, side, qty, eph: u8, n: u8| {
            seal_order_with(
                &Order { user: [user; 32], market, side, qty, limit_price: 0 },
                &cluster.public_bytes(),
                StaticSecret::from([eph; 32]),
                [n; 12],
            )
        };
        let orders = vec![seal(1, Side::Yes, 300, 10, 1), seal(2, Side::No, 100, 11, 2)];
        let params = EpochParams { market, epoch: 1, batch_id: 1 };
        let batch = process_epoch(&cluster, &mut mxe, &committee, &orders, &params).unwrap();
        (committee, batch)
    }

    #[test]
    fn ed25519_instruction_is_byte_compatible_with_on_chain_parser() {
        let (committee, batch) = make_batch();
        for (i, sig) in batch.signatures.iter().enumerate() {
            let ix = ed25519_precompile_ix(sig, &batch.frame);
            assert_eq!(ix.program_id, ED25519_PROGRAM_ID);
            let (pk, msg) = parse_ed25519(&ix.data);
            assert_eq!(pk, committee.member_pubkey(i).unwrap().to_vec());
            assert_eq!(msg, batch.frame, "signed message must be the exact frame");
        }
    }

    #[test]
    fn submit_batch_instruction_has_expected_shape() {
        let (_committee, batch) = make_batch();
        let market = Address::new_from_array([7u8; 32]);
        let config = MarketConfig::new(
            market,
            Address::new_from_array([20u8; 32]),
            Address::new_from_array([21u8; 32]),
            Address::new_from_array([22u8; 32]),
        );
        let relayer = Address::new_from_array([30u8; 32]);
        let positions: Vec<Address> = batch
            .fill_owners()
            .iter()
            .map(|o| config.position_pda(o))
            .collect();
        let ix = submit_batch_ix(&config, &relayer, &batch.frame, &positions);

        assert_eq!(ix.program_id, SHADOWSTATE_PROGRAM_ID);
        assert_eq!(ix.data[0], ix::SUBMIT_BATCH);
        assert_eq!(&ix.data[1..], &batch.frame[..]);
        // 9 fixed accounts + one position per fill (2 fills).
        assert_eq!(ix.accounts.len(), 9 + 2);
        assert!(ix.accounts[0].is_signer, "relayer must sign");
        assert!(ix.accounts[1].is_writable, "market must be writable");
        assert_eq!(ix.accounts[3].pubkey, INSTRUCTIONS_SYSVAR);
        assert_eq!(ix.accounts[8].pubkey, TOKEN_2022_PROGRAM_ID);
        // Positions are the per-fill PDAs in fill order.
        assert_eq!(ix.accounts[9].pubkey, config.position_pda(&[1u8; 32]));
        assert_eq!(ix.accounts[10].pubkey, config.position_pda(&[2u8; 32]));
    }

    #[test]
    fn full_instruction_list_is_sigs_then_submit() {
        let (_c, batch) = make_batch();
        let config = MarketConfig::new(
            Address::new_from_array([7u8; 32]),
            Address::new_from_array([20u8; 32]),
            Address::new_from_array([21u8; 32]),
            Address::new_from_array([22u8; 32]),
        );
        let relayer = Address::new_from_array([30u8; 32]);
        let ixs = build_instructions(&config, &relayer, &batch);
        // 2 ed25519 + 1 submit.
        assert_eq!(ixs.len(), 3);
        assert_eq!(ixs[0].program_id, ED25519_PROGRAM_ID);
        assert_eq!(ixs[1].program_id, ED25519_PROGRAM_ID);
        assert_eq!(ixs[2].program_id, SHADOWSTATE_PROGRAM_ID);
    }

    #[test]
    fn program_id_matches_on_chain_constant() {
        // The relay must target exactly the deployed program. Byte-check the embedded ID.
        assert_eq!(&SHADOWSTATE_PROGRAM_ID.as_array()[..11], b"ShadowState");
    }

    #[test]
    fn trusted_transaction_is_authority_signed_with_no_ed25519() {
        use crate::relayer::{ClearedBatch, Relayer, SlotRegistry};
        // Re-derive a cleared batch into a RelayedBatch (no committee), then build the trusted tx.
        let cleared = ClearedBatch::from_gateway(
            [7u8; 32], 1, 1, protocol::DIRECTION_YES_HEAVY, 200, 300, 100, &[0, 1], &[300, 100], 2,
        )
        .unwrap();
        let mut reg = SlotRegistry::default();
        reg.record(0, [1u8; 32]);
        reg.record(1, [2u8; 32]);
        let relayed = Relayer::new(4, 1).clear(&cleared, &reg).unwrap();

        let config = MarketConfig::new(
            Address::new_from_array([7u8; 32]),
            Address::new_from_array([20u8; 32]),
            Address::new_from_array([21u8; 32]),
            Address::new_from_array([22u8; 32]),
        );
        let authority = Keypair::new();
        let tx = build_trusted_transaction(&config, &authority, &relayed, solana_hash::Hash::new_from_array([9u8; 32]));

        // Exactly one instruction (no Ed25519 precompile instructions), authority-signed.
        assert_eq!(tx.message.instructions.len(), 1);
        assert_eq!(tx.signatures.len(), 1);
        assert!(tx.is_signed());
        assert_eq!(tx.message.account_keys[0], authority.pubkey());
        // The single instruction is SubmitBatchTrusted with the frame payload + 2 position accounts.
        let ix = submit_batch_trusted_ix(
            &config,
            &authority.pubkey(),
            &relayed.frame,
            &[config.position_pda(&[1u8; 32]), config.position_pda(&[2u8; 32])],
        );
        assert_eq!(ix.data[0], protocol::ids::ix::SUBMIT_BATCH_TRUSTED);
        assert_eq!(ix.accounts.len(), 2 + 2);
        assert!(ix.accounts[0].is_signer);
    }
}
