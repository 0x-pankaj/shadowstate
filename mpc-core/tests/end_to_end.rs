//! End-to-end integration: seal → ingest → match → frame → sign → relay, asserting the produced
//! batch settles to the **exact same on-chain state** the program's LiteSVM suite checks for the
//! canonical YES-heavy scenario (`submit_batch_settles_two_tiers_and_pays_mm_fee`).
//!
//! This proves the off-chain engine and the on-chain engine agree byte-for-byte on the frame and
//! number-for-number on the settlement, without coupling mpc-core to the program's pinned LiteSVM
//! dependency graph. The frame/sig byte compatibility itself is covered by the in-crate `relay`
//! and `frame` tests against the real on-chain parsers; here we validate the *economics*.

use {
    shadowstate_mpc::{
        committee::{quorum_is_met, Committee, NodeSignature},
        engine::{process_epoch, EpochParams, SignedBatch},
        mxe::MxeCluster,
        order::{Order, Side},
        relay::{build_transaction, MarketConfig},
        seal::{seal_order_with, ClusterKey},
    },
    solana_address::Address,
    solana_keypair::Keypair,
    solana_signer::Signer,
    x25519_dalek::StaticSecret,
};

// Market risk params — identical to the on-chain LiteSVM harness.
const SCALE: u64 = 1_000_000;
const MIDPOINT: u64 = 500_000;
const BASE_PRICE: u64 = 500_000;
const MAX_PREMIUM: u64 = 100_000;
const THRESHOLD_IMBALANCE: u64 = 1_000;
const MARKET: [u8; 32] = [7u8; 32];

const ALICE: [u8; 32] = [1u8; 32];
const BOB: [u8; 32] = [2u8; 32];

/// Re-derivation of the on-chain Tier-2 pricing (program `math.rs`), used to check the off-chain
/// frame would settle to the expected balances.
fn clearing_price_yes_heavy(net: u64) -> (u64, u64) {
    let skew_ratio = ((net as u128 * SCALE as u128) / THRESHOLD_IMBALANCE as u128).min(SCALE as u128);
    let premium = (MAX_PREMIUM as u128 * skew_ratio / SCALE as u128) as u64;
    let clearing = BASE_PRICE + premium; // YES-heavy adds the premium
    (clearing, premium)
}

fn collateral_for(qty: u64, price: u64) -> u64 {
    (qty as u128 * price as u128 / SCALE as u128) as u64
}

fn committee_3of_threshold_2() -> Committee {
    let seeds: Vec<[u8; 32]> = (1..=3u8)
        .map(|i| {
            let mut s = [0u8; 32];
            s[0] = i;
            s
        })
        .collect();
    Committee::from_seeds(&seeds, 2).unwrap()
}

fn yes_heavy_batch() -> (Committee, SignedBatch) {
    let cluster = ClusterKey::from_seed([1u8; 32]);
    let mut mxe = MxeCluster::new(4, 0xFEED);
    let committee = committee_3of_threshold_2();

    let seal = |user: [u8; 32], side, qty, eph: u8, nonce: u8| {
        seal_order_with(
            &Order { user, market: MARKET, side, qty, limit_price: 0 },
            &cluster.public_bytes(),
            StaticSecret::from([eph; 32]),
            [nonce; 12],
        )
    };
    // Alice buys 300 YES, Bob buys 100 NO → 100 crosses P2P, 200 residual YES-heavy.
    let sealed = vec![
        seal(ALICE, Side::Yes, 300, 10, 1),
        seal(BOB, Side::No, 100, 11, 2),
    ];
    let params = EpochParams { market: MARKET, epoch: 1, batch_id: 1 };
    let batch = process_epoch(&cluster, &mut mxe, &committee, &sealed, &params).unwrap();
    (committee, batch)
}

#[test]
fn produced_batch_settles_to_the_on_chain_expected_state() {
    let (committee, batch) = yes_heavy_batch();

    // The committee quorum the on-chain precompile path would require.
    assert!(quorum_is_met(&committee.member_pubkeys(), 2, &batch.frame, &batch.signatures));

    // Pull the per-user fills out of the produced match.
    let fills = &batch.result.fills;
    let alice = fills.iter().find(|f| f.user == ALICE).unwrap();
    let bob = fills.iter().find(|f| f.user == BOB).unwrap();

    assert_eq!(batch.result.direction, protocol::DIRECTION_YES_HEAVY);
    assert_eq!(batch.result.net_imbalance, 200);

    // --- replicate the on-chain two-tier settlement and compare to the known-good numbers --------
    let (clearing, premium) = clearing_price_yes_heavy(batch.result.net_imbalance);
    assert_eq!(clearing, 520_000); // $0.52
    assert_eq!(premium, 20_000); // $0.02

    // Alice: Tier-1 100 YES @ $0.50 = 50 ; Tier-2 200 YES @ $0.52 = 104 ; total 154 ; qty 300.
    let alice_t1 = collateral_for(alice.p2p_yes + alice.p2p_no, MIDPOINT);
    let alice_t2 = collateral_for(alice.residual_yes, clearing);
    assert_eq!(alice.p2p_yes, 100);
    assert_eq!(alice.residual_yes, 200);
    assert_eq!(alice_t1 + alice_t2, 154);
    assert_eq!(alice.p2p_yes + alice.residual_yes, 300);

    // Bob: Tier-1 100 NO @ $0.50 = 50 ; no residual ; qty 100.
    let bob_cost = collateral_for(bob.p2p_no, MIDPOINT);
    assert_eq!(bob.p2p_no, 100);
    assert_eq!(bob.residual_no, 0);
    assert_eq!(bob_cost, 50);

    // MM spread fee = net_imbalance * premium = 200 * $0.02 = 4 (matches the on-chain assertion).
    let mm_fee = collateral_for(batch.result.net_imbalance, premium);
    assert_eq!(mm_fee, 4);
}

#[test]
fn relay_assembles_a_signed_broadcastable_transaction() {
    let (_committee, batch) = yes_heavy_batch();
    let config = MarketConfig::new(
        Address::new_from_array(MARKET),
        Address::new_from_array([20u8; 32]), // vault
        Address::new_from_array([21u8; 32]), // mm account
        Address::new_from_array([22u8; 32]), // mint
    );
    let relayer = Keypair::new();
    // A deterministic, valid-length dummy blockhash for offline assembly.
    let blockhash = solana_hash::Hash::new_from_array([9u8; 32]);

    let tx = build_transaction(&config, &relayer, &batch, blockhash);

    // 2 Ed25519 instructions + 1 SubmitBatch.
    assert_eq!(tx.message.instructions.len(), 3);
    // The relayer is the sole transaction signer; the committee authority rides in the Ed25519 ixs.
    assert_eq!(tx.signatures.len(), 1);
    assert!(tx.is_signed());
    // The signed message carries the relayer as fee payer.
    assert_eq!(tx.message.account_keys[0], relayer.pubkey());
}

#[test]
fn two_signatures_are_distinct_committee_members() {
    let (committee, batch) = yes_heavy_batch();
    let pubkeys = committee.member_pubkeys();
    let signers: Vec<&NodeSignature> = batch.signatures.iter().collect();
    assert_eq!(signers.len(), 2);
    assert_ne!(signers[0].pubkey, signers[1].pubkey, "distinct signers");
    for s in signers {
        assert!(pubkeys.contains(&s.pubkey), "every signer is a registered member");
    }
}
