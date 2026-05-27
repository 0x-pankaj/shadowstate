//! The Frequent-Batch-Auction ingestion loop.
//!
//! A strict `tokio::time::interval` fires every **1200 ms** (`EPOCH_SLOTS` × ~400 ms/slot). Each tick
//! fetches the sealed orders from the user ingestion accounts, runs one secure epoch
//! ([`process_epoch`]), and hands the resulting signed batch to a sink (typically the relay, which
//! builds and broadcasts the `SubmitBatch` transaction).
//!
//! The order *source* is abstracted by [`IngestionSource`] so the loop is testable offline. The
//! production source is a thin adapter over Solana RPC `getProgramAccounts` that returns the raw
//! sealed-order bytes of each ingestion account — it implements this one trait and slots straight
//! in, keeping a heavy RPC client out of this crate's default dependency set.

use {
    crate::{
        committee::Committee,
        engine::{process_epoch, EpochParams, SignedBatch},
        error::Result,
        mxe::MxeCluster,
        seal::ClusterKey,
    },
    protocol::EPOCH_SLOTS,
    std::time::Duration,
};

/// Approximate Solana slot time. The FBA cadence is `EPOCH_SLOTS` of these.
pub const SLOT_DURATION: Duration = Duration::from_millis(400);

/// The 1200 ms Frequent-Batch-Auction tick (`EPOCH_SLOTS` × [`SLOT_DURATION`]).
pub const FBA_TICK: Duration = Duration::from_millis(EPOCH_SLOTS * 400);

/// Source of sealed orders for a market. Implemented by the RPC adapter in production and by an
/// in-memory source in tests.
pub trait IngestionSource {
    /// Fetch the current sealed-order blobs for `market` (one per ingestion account).
    fn fetch(
        &self,
        market: &[u8; 32],
    ) -> impl std::future::Future<Output = Result<Vec<Vec<u8>>>>;
}

/// Destination for finalized batches (e.g. the relay/broadcast path).
pub trait BatchSink {
    /// Deliver one finalized, signed batch.
    fn deliver(
        &mut self,
        batch: SignedBatch,
    ) -> impl std::future::Future<Output = Result<()>>;
}

/// The stateful FBA engine: owns the cluster key, MXE matrix, committee, and the monotonic epoch
/// counter, and drives the auction loop.
pub struct FbaEngine {
    cluster_key: ClusterKey,
    mxe: MxeCluster,
    committee: Committee,
    market: [u8; 32],
    epoch: u64,
}

impl FbaEngine {
    /// Construct an engine. `last_settled_epoch` is the highest epoch already settled on-chain
    /// (`MarketState.last_epoch`); the first tick uses `last_settled_epoch + 1`, satisfying the
    /// on-chain replay guard `epoch > last_epoch`.
    pub fn new(
        cluster_key: ClusterKey,
        mxe: MxeCluster,
        committee: Committee,
        market: [u8; 32],
        last_settled_epoch: u64,
    ) -> Self {
        Self {
            cluster_key,
            mxe,
            committee,
            market,
            epoch: last_settled_epoch,
        }
    }

    /// The epoch the most recent tick produced.
    #[inline]
    pub fn current_epoch(&self) -> u64 {
        self.epoch
    }

    /// Run one auction tick: fetch, match, sign. Returns `None` for an empty book (no fills) so the
    /// caller can skip a pointless on-chain submission while the epoch still advances.
    pub async fn tick<S: IngestionSource>(&mut self, source: &S) -> Result<Option<SignedBatch>> {
        let sealed = source.fetch(&self.market).await?;
        // Advance first so every produced frame carries a strictly-increasing epoch, even for
        // empty ticks — keeps the off-chain counter monotonic with the on-chain replay guard.
        self.epoch += 1;
        let params = EpochParams {
            market: self.market,
            epoch: self.epoch,
            batch_id: self.epoch,
        };
        let batch = process_epoch(&self.cluster_key, &mut self.mxe, &self.committee, &sealed, &params)?;
        Ok(if batch.is_empty() { None } else { Some(batch) })
    }

    /// Drive the auction on the strict 1200 ms cadence, delivering each non-empty batch to `sink`.
    /// Runs forever when `max_ticks` is `None`, or exactly `max_ticks` ticks otherwise (the bounded
    /// form is what tests and graceful shutdowns use).
    pub async fn run<S: IngestionSource, K: BatchSink>(
        &mut self,
        source: &S,
        sink: &mut K,
        max_ticks: Option<u64>,
    ) -> Result<()> {
        let mut interval = tokio::time::interval(FBA_TICK);
        // If a tick's work overruns the period, skip missed ticks rather than burst-catch-up — the
        // FBA wants the *next* auction, not a backlog replay.
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        let mut count = 0u64;
        loop {
            interval.tick().await;
            if let Some(batch) = self.tick(source).await? {
                sink.deliver(batch).await?;
            }
            count += 1;
            if max_ticks.is_some_and(|m| count >= m) {
                break;
            }
        }
        Ok(())
    }
}

/// In-memory order source for tests: returns a fixed set of sealed blobs every tick.
pub struct MemoryIngestionSource {
    sealed: Vec<Vec<u8>>,
}

impl MemoryIngestionSource {
    /// Wrap a fixed set of sealed-order blobs.
    pub fn new(sealed: Vec<Vec<u8>>) -> Self {
        Self { sealed }
    }
}

impl IngestionSource for MemoryIngestionSource {
    async fn fetch(&self, _market: &[u8; 32]) -> Result<Vec<Vec<u8>>> {
        Ok(self.sealed.clone())
    }
}

/// Collecting sink for tests: retains every delivered batch.
#[derive(Default)]
pub struct CollectingSink {
    /// All batches delivered so far, in order.
    pub batches: Vec<SignedBatch>,
}

impl BatchSink for CollectingSink {
    async fn deliver(&mut self, batch: SignedBatch) -> Result<()> {
        self.batches.push(batch);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            order::{Order, Side},
            seal::seal_order_with,
        },
        protocol::{validate_frame_len, DIRECTION_YES_HEAVY},
        x25519_dalek::StaticSecret,
    };

    const MARKET: [u8; 32] = [7u8; 32];

    fn engine_and_source() -> (FbaEngine, MemoryIngestionSource) {
        let cluster = ClusterKey::from_seed([1u8; 32]);
        let mxe = MxeCluster::new(4, 0x55);
        let seeds: Vec<[u8; 32]> = (1..=3u8)
            .map(|i| {
                let mut s = [0u8; 32];
                s[0] = i;
                s
            })
            .collect();
        let committee = Committee::from_seeds(&seeds, 2).unwrap();

        let seal = |user: u8, side, qty, eph: u8, n: u8| {
            seal_order_with(
                &Order { user: [user; 32], market: MARKET, side, qty, limit_price: 0 },
                &cluster.public_bytes(),
                StaticSecret::from([eph; 32]),
                [n; 12],
            )
        };
        let sealed = vec![seal(1, Side::Yes, 300, 10, 1), seal(2, Side::No, 100, 11, 2)];
        let source = MemoryIngestionSource::new(sealed);
        let engine = FbaEngine::new(cluster, mxe, committee, MARKET, 0);
        (engine, source)
    }

    #[test]
    fn fba_tick_is_1200ms() {
        assert_eq!(FBA_TICK, Duration::from_millis(1200));
    }

    #[tokio::test(start_paused = true)]
    async fn loop_produces_one_batch_per_tick_with_increasing_epochs() {
        let (mut engine, source) = engine_and_source();
        let mut sink = CollectingSink::default();

        // Paused clock auto-advances through the interval; three ticks complete near-instantly.
        engine.run(&source, &mut sink, Some(3)).await.unwrap();

        assert_eq!(sink.batches.len(), 3, "one batch per tick");
        assert_eq!(engine.current_epoch(), 3);
        let mut last = 0u64;
        for b in &sink.batches {
            let h = validate_frame_len(&b.frame).unwrap();
            assert_eq!(h.market, MARKET);
            assert_eq!(h.direction, DIRECTION_YES_HEAVY);
            assert_eq!(h.net_imbalance, 200);
            assert!(h.epoch > last, "epochs must strictly increase (on-chain replay guard)");
            last = h.epoch;
            assert_eq!(b.signatures.len(), 2);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn empty_book_advances_epoch_but_yields_no_batch() {
        let cluster = ClusterKey::from_seed([1u8; 32]);
        let mxe = MxeCluster::new(2, 0);
        let committee = Committee::from_seeds(&[[1u8; 32]], 1).unwrap();
        let mut engine = FbaEngine::new(cluster, mxe, committee, MARKET, 7);
        let source = MemoryIngestionSource::new(vec![]);
        let mut sink = CollectingSink::default();

        engine.run(&source, &mut sink, Some(2)).await.unwrap();
        assert!(sink.batches.is_empty(), "empty books produce no submission");
        assert_eq!(engine.current_epoch(), 9, "but the epoch still advances past last_epoch");
    }
}
