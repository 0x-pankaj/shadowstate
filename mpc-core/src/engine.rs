//! The per-epoch pipeline: unseal → match → frame → sign. One call models one FBA tick's secure
//! computation, end to end.

use {
    crate::{
        committee::{Committee, NodeSignature},
        error::{MpcError, Result},
        frame::build_frame,
        mxe::{MatchResult, MxeCluster},
        seal::ClusterKey,
    },
};

/// The market + sequencing parameters for one batch auction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpochParams {
    /// Target `MarketState` PDA the frame settles against.
    pub market: [u8; 32],
    /// Strictly-increasing epoch (on-chain replay guard requires `epoch > last_epoch`).
    pub epoch: u64,
    /// Monotonic batch id (bookkeeping / event correlation).
    pub batch_id: u64,
}

/// A finalized, signed batch ready for the relay.
#[derive(Debug, Clone)]
pub struct SignedBatch {
    /// Canonical frame bytes (`BatchHeader ++ [UserFill]`) — the signed message and the
    /// `SubmitBatch` instruction payload (after the discriminator).
    pub frame: Vec<u8>,
    /// Threshold committee signatures over `frame`.
    pub signatures: Vec<NodeSignature>,
    /// The matching result, retained so the relay can derive the per-fill `UserPosition` accounts.
    pub result: MatchResult,
}

impl SignedBatch {
    /// `true` if the batch settled no fills (an empty FBA tick). Relayers typically skip submitting
    /// these to save fees while still advancing their local epoch counter.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.result.fills.is_empty()
    }

    /// The ordered list of fill owners — the order the relay must list `UserPosition` accounts in.
    pub fn fill_owners(&self) -> Vec<[u8; 32]> {
        self.result.fills.iter().map(|f| f.user).collect()
    }
}

/// Run one batch auction: open every sealed order with the cluster key, privately match, assemble
/// the frame, and collect a threshold of committee signatures.
///
/// Every order must belong to `params.market`; a foreign-market order is a caller bug (the relayer
/// filters ingestion accounts by market upstream) and yields [`MpcError::MarketMismatch`].
pub fn process_epoch(
    cluster_key: &ClusterKey,
    mxe: &mut MxeCluster,
    committee: &Committee,
    sealed_orders: &[Vec<u8>],
    params: &EpochParams,
) -> Result<SignedBatch> {
    // 1. Unseal inside the secure context.
    let mut orders = Vec::with_capacity(sealed_orders.len());
    for blob in sealed_orders {
        let order = cluster_key.open(blob)?;
        if order.market != params.market {
            return Err(MpcError::MarketMismatch);
        }
        orders.push(order);
    }

    // 2. Blinded matching matrix.
    let result = mxe.match_batch(&orders)?;

    // 3. Assemble the exact on-chain frame.
    let frame = build_frame(params.market, params.epoch, params.batch_id, &result);

    // 4. Threshold-sign across the node matrix.
    let signatures = committee.sign_threshold(&frame)?;

    Ok(SignedBatch {
        frame,
        signatures,
        result,
    })
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            committee::quorum_is_met,
            order::{Order, Side},
            seal::seal_order_with,
        },
        protocol::{validate_frame_len, DIRECTION_YES_HEAVY},
        x25519_dalek::StaticSecret,
    };

    const MARKET: [u8; 32] = [7u8; 32];

    fn sealed(cluster: &ClusterKey, user: u8, side: Side, qty: u64, eph: u8, nonce: u8) -> Vec<u8> {
        let order = Order {
            user: [user; 32],
            market: MARKET,
            side,
            qty,
            limit_price: 0,
        };
        seal_order_with(&order, &cluster.public_bytes(), StaticSecret::from([eph; 32]), [nonce; 12])
    }

    #[test]
    fn full_epoch_produces_a_valid_signed_batch() {
        let cluster = ClusterKey::from_seed([1u8; 32]);
        let mut mxe = MxeCluster::new(4, 0x1234);
        let seeds: Vec<[u8; 32]> = (1..=3u8)
            .map(|i| {
                let mut s = [0u8; 32];
                s[0] = i;
                s
            })
            .collect();
        let committee = Committee::from_seeds(&seeds, 2).unwrap();

        let orders = vec![
            sealed(&cluster, 1, Side::Yes, 300, 10, 1),
            sealed(&cluster, 2, Side::No, 100, 11, 2),
        ];
        let params = EpochParams {
            market: MARKET,
            epoch: 1,
            batch_id: 1,
        };
        let batch = process_epoch(&cluster, &mut mxe, &committee, &orders, &params).unwrap();

        let header = validate_frame_len(&batch.frame).unwrap();
        assert_eq!(header.market, MARKET);
        assert_eq!(header.direction, DIRECTION_YES_HEAVY);
        assert_eq!(header.net_imbalance, 200);
        assert_eq!(batch.signatures.len(), 2);
        assert!(quorum_is_met(&committee.member_pubkeys(), 2, &batch.frame, &batch.signatures));
        assert_eq!(batch.fill_owners(), vec![[1u8; 32], [2u8; 32]]);
    }

    #[test]
    fn foreign_market_order_is_rejected() {
        let cluster = ClusterKey::from_seed([1u8; 32]);
        let mut mxe = MxeCluster::new(2, 1);
        let committee = Committee::from_seeds(&[[1u8; 32]], 1).unwrap();
        let foreign = Order {
            user: [1u8; 32],
            market: [99u8; 32],
            side: Side::Yes,
            qty: 10,
            limit_price: 0,
        };
        let blob =
            seal_order_with(&foreign, &cluster.public_bytes(), StaticSecret::from([2u8; 32]), [3u8; 12]);
        let params = EpochParams {
            market: MARKET,
            epoch: 1,
            batch_id: 1,
        };
        let err = process_epoch(&cluster, &mut mxe, &committee, &[blob], &params).unwrap_err();
        assert_eq!(err, MpcError::MarketMismatch);
    }

    #[test]
    fn empty_epoch_is_a_valid_no_op_batch() {
        let cluster = ClusterKey::from_seed([1u8; 32]);
        let mut mxe = MxeCluster::new(3, 0);
        let committee = Committee::from_seeds(&[[1u8; 32]], 1).unwrap();
        let params = EpochParams {
            market: MARKET,
            epoch: 5,
            batch_id: 5,
        };
        let batch = process_epoch(&cluster, &mut mxe, &committee, &[], &params).unwrap();
        assert!(batch.is_empty());
        let header = validate_frame_len(&batch.frame).unwrap();
        assert_eq!(header.fill_count, 0);
        assert_eq!(header.net_imbalance, 0);
    }
}
