//! The production relayer — the bridge from Arcium's revealed clearing to on-chain settlement.
//!
//! In the live system the Arcium MXE (`encrypted-ixs::clear_batch`, driven by the `arcium-gateway`)
//! keeps the order book secret until the batch closes, then **reveals** the cleared result: the
//! aggregate economics plus the per-slot `(side, qty)`. The relayer's job is purely deterministic and
//! public from there: reconstruct the orders (slot → owner is recorded at ingest time, public under
//! the positions-public model), re-derive the per-user two-tier fills, **cross-check** them against
//! the gateway's revealed aggregates, and submit a committee-signed `SubmitBatch` to the Pinocchio
//! settlement program.
//!
//! Because the matching is deterministic given the orders, the relayer reuses the exact allocator the
//! local model uses ([`MxeCluster::match_batch`]); the gateway aggregates serve as an independent
//! cross-check, so a relayer/gateway disagreement is rejected rather than settled on bad data.

use crate::{
    committee::Committee,
    engine::SignedBatch,
    error::{MpcError, Result},
    frame::build_frame,
    mxe::{MatchResult, MxeCluster},
    order::{Order, Side},
};

/// One revealed slot of the cleared batch: a public order whose side/size were secret until clearing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotOrder {
    /// Public slot index (assigned by the gateway at ingest).
    pub slot: usize,
    /// Revealed side.
    pub side: Side,
    /// Revealed quantity.
    pub qty: u64,
}

/// The plaintext clearing the Arcium `clear_batch` circuit revealed — a mirror of the gateway's
/// `BatchCleared` event. The `total_*` / `net_imbalance` / `direction` fields are the gateway's
/// computed aggregates, used to cross-check the relayer's independent re-derivation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClearedBatch {
    pub market: [u8; 32],
    pub epoch: u64,
    pub batch_id: u64,
    pub direction: u8,
    pub net_imbalance: u64,
    pub total_yes: u64,
    pub total_no: u64,
    /// Active slots (one per ingested order), in slot order.
    pub slots: Vec<SlotOrder>,
}

impl ClearedBatch {
    /// Build from the gateway's fixed-array event shape (`sides[N]`, `qtys[N]`, `count`). Slots
    /// `0..count` with a non-zero quantity become active `SlotOrder`s.
    #[allow(clippy::too_many_arguments)]
    pub fn from_gateway(
        market: [u8; 32],
        epoch: u64,
        batch_id: u64,
        direction: u8,
        net_imbalance: u64,
        total_yes: u64,
        total_no: u64,
        sides: &[u8],
        qtys: &[u64],
        count: usize,
    ) -> Result<Self> {
        let mut slots = Vec::new();
        for i in 0..count {
            let qty = *qtys.get(i).ok_or(MpcError::GatewayDisagreement)?;
            if qty == 0 {
                continue;
            }
            let side = match sides.get(i) {
                Some(0) => Side::Yes,
                Some(1) => Side::No,
                _ => return Err(MpcError::GatewayDisagreement),
            };
            slots.push(SlotOrder { slot: i, side, qty });
        }
        Ok(ClearedBatch {
            market,
            epoch,
            batch_id,
            direction,
            net_imbalance,
            total_yes,
            total_no,
            slots,
        })
    }
}

/// Maps a public ingest slot to the owner whose order filled it. The relayer records this as it
/// submits each `ingest_order` (the slot the gateway assigns is public); at clearing it resolves
/// revealed slots back to owners so the on-chain `UserPosition` accounts can be addressed.
#[derive(Debug, Default, Clone)]
pub struct SlotRegistry {
    owners: std::collections::HashMap<usize, [u8; 32]>,
}

impl SlotRegistry {
    /// Record the owner the gateway assigned to `slot`.
    pub fn record(&mut self, slot: usize, owner: [u8; 32]) {
        self.owners.insert(slot, owner);
    }

    /// The owner of `slot`, if recorded.
    pub fn owner(&self, slot: usize) -> Option<[u8; 32]> {
        self.owners.get(&slot).copied()
    }
}

/// A frame the relayer has re-derived from a revealed batch and cross-checked against the gateway —
/// ready for either settlement path. For the **trusted** path (`SubmitBatchTrusted`) this is all that
/// is needed (no committee signatures); the **committee** path wraps it with signatures.
#[derive(Debug, Clone)]
pub struct RelayedBatch {
    /// Canonical settlement frame bytes (`BatchHeader ++ [UserFill]`).
    pub frame: Vec<u8>,
    /// The re-derived per-user fills (drives the `UserPosition` account list).
    pub result: MatchResult,
}

impl RelayedBatch {
    /// The ordered fill owners — the order the on-chain `UserPosition` accounts must be listed in.
    pub fn fill_owners(&self) -> Vec<[u8; 32]> {
        self.result.fills.iter().map(|f| f.user).collect()
    }
}

/// Turns a [`ClearedBatch`] into a settlement frame ready for `relay::build_transaction` (committee
/// path) or `relay::build_trusted_transaction` (gateway-authority path).
pub struct Relayer {
    mxe: MxeCluster,
}

impl Relayer {
    /// A relayer with a deterministic allocator. `nodes`/`blind_seed` only parameterise the (now
    /// public) re-derivation; they need not match the live cluster.
    pub fn new(nodes: usize, blind_seed: u64) -> Self {
        Self {
            mxe: MxeCluster::new(nodes, blind_seed),
        }
    }

    /// Re-derive + cross-check a revealed batch into a [`RelayedBatch`] (no signing). This is the
    /// **trusted-path** output: reconstruct orders from `cleared.slots` + `registry`, re-derive the
    /// two-tier fills, cross-check against the gateway aggregates, and assemble the frame.
    pub fn clear(&mut self, cleared: &ClearedBatch, registry: &SlotRegistry) -> Result<RelayedBatch> {
        // 1. Reconstruct the (now public) orders, resolving each slot to its owner.
        let mut orders = Vec::with_capacity(cleared.slots.len());
        for s in &cleared.slots {
            let owner = registry.owner(s.slot).ok_or(MpcError::SlotOwnerMissing(s.slot))?;
            orders.push(Order {
                user: owner,
                market: cleared.market,
                side: s.side,
                qty: s.qty,
                limit_price: 0,
            });
        }

        // 2. Re-derive the deterministic two-tier match.
        let result = self.mxe.match_batch(&orders)?;

        // 3. Cross-check against the gateway's revealed aggregates — reject on any disagreement.
        if result.direction != cleared.direction
            || result.net_imbalance != cleared.net_imbalance
            || result.total_yes != cleared.total_yes
            || result.total_no != cleared.total_no
        {
            return Err(MpcError::GatewayDisagreement);
        }

        // 4. Assemble the settlement frame.
        let frame = build_frame(cleared.market, cleared.epoch, cleared.batch_id, &result);
        Ok(RelayedBatch { frame, result })
    }

    /// **Committee path**: [`clear`](Self::clear) + a threshold of committee signatures. The returned
    /// [`SignedBatch`] feeds `relay::build_transaction` (`SubmitBatch`).
    pub fn settle(
        &mut self,
        cleared: &ClearedBatch,
        registry: &SlotRegistry,
        committee: &Committee,
    ) -> Result<SignedBatch> {
        let relayed = self.clear(cleared, registry)?;
        let signatures = committee.sign_threshold(&relayed.frame)?;
        Ok(SignedBatch {
            frame: relayed.frame,
            signatures,
            result: relayed.result,
        })
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::committee::{quorum_is_met, Committee},
        protocol::{validate_frame_len, DIRECTION_YES_HEAVY},
    };

    const MARKET: [u8; 32] = [7u8; 32];
    const ALICE: [u8; 32] = [1u8; 32];
    const BOB: [u8; 32] = [2u8; 32];

    fn committee() -> Committee {
        let seeds: Vec<[u8; 32]> = (1..=3u8)
            .map(|i| {
                let mut s = [0u8; 32];
                s[0] = i;
                s
            })
            .collect();
        Committee::from_seeds(&seeds, 2).unwrap()
    }

    /// Canonical YES-heavy clearing (alice 300 YES @ slot 0, bob 100 NO @ slot 1).
    fn canonical() -> (ClearedBatch, SlotRegistry) {
        let cleared = ClearedBatch::from_gateway(
            MARKET, 1, 1, DIRECTION_YES_HEAVY, 200, 300, 100,
            &[0, 1], &[300, 100], 2,
        )
        .unwrap();
        let mut reg = SlotRegistry::default();
        reg.record(0, ALICE);
        reg.record(1, BOB);
        (cleared, reg)
    }

    #[test]
    fn settles_revealed_batch_into_signed_frame() {
        let (cleared, reg) = canonical();
        let c = committee();
        let mut relayer = Relayer::new(4, 0xABCD);
        let batch = relayer.settle(&cleared, &reg, &c).unwrap();

        // The produced frame is on-chain valid and quorum-signed.
        let header = validate_frame_len(&batch.frame).unwrap();
        assert_eq!(header.market, MARKET);
        assert_eq!(header.direction, DIRECTION_YES_HEAVY);
        assert_eq!(header.net_imbalance, 200);
        assert!(quorum_is_met(&c.member_pubkeys(), 2, &batch.frame, &batch.signatures));

        // Re-derived fills reproduce the canonical settlement (alice 100 P2P + 200 residual YES).
        let alice = batch.result.fills.iter().find(|f| f.user == ALICE).unwrap();
        assert_eq!(alice.p2p_yes, 100);
        assert_eq!(alice.residual_yes, 200);
        let bob = batch.result.fills.iter().find(|f| f.user == BOB).unwrap();
        assert_eq!(bob.p2p_no, 100);
        assert_eq!(batch.fill_owners(), vec![ALICE, BOB]);
    }

    #[test]
    fn rejects_gateway_aggregate_disagreement() {
        let (mut cleared, reg) = canonical();
        cleared.net_imbalance = 199; // gateway claims a net the slots don't support
        let mut relayer = Relayer::new(4, 1);
        let err = relayer.settle(&cleared, &reg, &committee()).unwrap_err();
        assert_eq!(err, MpcError::GatewayDisagreement);
    }

    #[test]
    fn rejects_missing_slot_owner() {
        let (cleared, _) = canonical();
        let empty = SlotRegistry::default(); // never recorded the slot owners
        let mut relayer = Relayer::new(4, 1);
        let err = relayer.settle(&cleared, &empty, &committee()).unwrap_err();
        assert_eq!(err, MpcError::SlotOwnerMissing(0));
    }

    #[test]
    fn from_gateway_skips_empty_slots_and_rejects_bad_side() {
        // count 3 but slot 2 is empty (qty 0) → 2 active slots.
        let ok = ClearedBatch::from_gateway(
            MARKET, 1, 1, DIRECTION_YES_HEAVY, 200, 300, 100,
            &[0, 1, 0], &[300, 100, 0], 3,
        )
        .unwrap();
        assert_eq!(ok.slots.len(), 2);
        // An invalid side tag is a gateway disagreement.
        let bad = ClearedBatch::from_gateway(MARKET, 1, 1, 0, 0, 0, 0, &[9], &[5], 1);
        assert_eq!(bad.unwrap_err(), MpcError::GatewayDisagreement);
    }
}
