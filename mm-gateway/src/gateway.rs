//! The gateway orchestrator: wires the WS event stream → delta-hedger → venue, and exposes the
//! risk-parameter retune portal. Generic over the three network boundaries so it runs against real
//! adapters in production and mocks in tests.

use {
    crate::{
        error::Result,
        hedge::{compute_hedge, HedgeOrder},
        portal::update_risk_tx,
        risk::{MarketConditions, RiskParams, RiskPolicy},
        rpc::ChainSubmitter,
        stream::EventStream,
        venue::{VenueClient, VenueOrderAck},
    },
    solana_address::Address,
    solana_keypair::Keypair,
};

/// One hedged settlement: the offsetting order and the venue's acknowledgement.
#[derive(Debug, Clone)]
pub struct HedgeOutcome {
    /// The order computed and sent.
    pub order: HedgeOrder,
    /// The venue's response.
    pub ack: VenueOrderAck,
}

/// The institutional liquidity gateway. Drives hedging off a settlement stream and retunes on-chain
/// risk parameters through the authorized MM wallet.
pub struct Gateway<S, V, C> {
    stream: S,
    venue: V,
    submitter: C,
    authority: Keypair,
    market: Address,
    params: RiskParams,
}

impl<S, V, C> Gateway<S, V, C>
where
    S: EventStream,
    V: VenueClient,
    C: ChainSubmitter,
{
    /// Assemble a gateway. `authority` is the MM admin wallet; `market` its `MarketState` PDA;
    /// `initial_params` the risk parameters currently live on-chain (kept in sync by `retune`).
    pub fn new(
        stream: S,
        venue: V,
        submitter: C,
        authority: Keypair,
        market: Address,
        initial_params: RiskParams,
    ) -> Self {
        Self {
            stream,
            venue,
            submitter,
            authority,
            market,
            params: initial_params,
        }
    }

    /// The risk parameters the gateway believes are live on-chain.
    #[inline]
    pub fn params(&self) -> RiskParams {
        self.params
    }

    /// Run the hedging loop: for each settlement event, compute the offsetting hedge (if the MM
    /// absorbed a residual) and dispatch it to the venue. Processes at most `max_events` events
    /// (`None` = until the stream closes). Returns every hedge placed.
    pub async fn run(&mut self, max_events: Option<u64>) -> Result<Vec<HedgeOutcome>> {
        let mut outcomes = Vec::new();
        let mut processed = 0u64;
        while let Some(event) = self.stream.next_event().await? {
            if let Some(order) = compute_hedge(&event, &self.params)? {
                let ack = self.venue.place(&order).await?;
                outcomes.push(HedgeOutcome { order, ack });
            }
            processed += 1;
            if max_events.is_some_and(|m| processed >= m) {
                break;
            }
        }
        Ok(outcomes)
    }

    /// The parameter modification portal: compute target params from live conditions via `policy`,
    /// submit an on-chain `UpdateRiskParams` transaction signed by the MM authority, and adopt the
    /// new params locally. Returns the transaction signature.
    pub async fn retune<P: RiskPolicy>(
        &mut self,
        conditions: &MarketConditions,
        policy: &P,
    ) -> Result<String> {
        let target = policy.target(conditions);
        target.validate()?;
        let blockhash = self.submitter.latest_blockhash().await?;
        let tx = update_risk_tx(&self.authority, &self.market, &target, blockhash)?;
        let signature = self.submitter.submit_transaction(&tx).await?;
        self.params = target;
        Ok(signature)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            event::SettlementEvent,
            hedge::HedgeSide,
            portal::market_pda,
            risk::LinearVolPolicy,
            rpc::MockSubmitter,
            stream::MemoryStream,
            venue::MockVenue,
        },
        solana_hash::Hash,
        solana_signer::Signer,
    };

    fn canonical_params() -> RiskParams {
        RiskParams {
            base_oracle_price: 500_000,
            max_skew_premium: 100_000,
            imbalance_threshold: 1_000,
        }
    }

    fn event(direction: u8, net: u64, epoch: u64) -> SettlementEvent {
        SettlementEvent {
            market: [7u8; 32],
            epoch,
            batch_id: epoch,
            direction,
            net_imbalance: net,
            p2p_volume: 0,
        }
    }

    fn gateway(events: Vec<SettlementEvent>) -> Gateway<MemoryStream, MockVenue, MockSubmitter> {
        let authority = Keypair::new();
        let market = market_pda(&authority.pubkey(), &crate::portal::SHADOWSTATE_PROGRAM_ID);
        Gateway::new(
            MemoryStream::new(events),
            MockVenue::new("kalshi"),
            MockSubmitter::new(Hash::new_from_array([5u8; 32])),
            authority,
            market,
            canonical_params(),
        )
    }

    #[tokio::test]
    async fn hedges_each_residual_with_correct_offsetting_side() {
        let events = vec![
            event(protocol::DIRECTION_YES_HEAVY, 200, 1),
            event(protocol::DIRECTION_NO_HEAVY, 150, 2),
            event(protocol::DIRECTION_YES_HEAVY, 0, 3), // no residual → no hedge
        ];
        let mut gw = gateway(events);
        let outcomes = gw.run(None).await.unwrap();

        // Two residual batches → two hedges; the zero-residual batch is skipped.
        assert_eq!(outcomes.len(), 2);
        // YES-heavy → buy YES 200 @ 520_000.
        assert_eq!(outcomes[0].order.side, HedgeSide::Yes);
        assert_eq!(outcomes[0].order.qty, 200);
        assert_eq!(outcomes[0].order.limit_price, 520_000);
        // NO-heavy → buy NO 150.
        assert_eq!(outcomes[1].order.side, HedgeSide::No);
        assert_eq!(outcomes[1].order.qty, 150);
        assert_eq!(outcomes[0].ack.filled_qty, 200);
    }

    #[tokio::test]
    async fn max_events_bounds_the_loop() {
        let events = vec![
            event(protocol::DIRECTION_YES_HEAVY, 100, 1),
            event(protocol::DIRECTION_YES_HEAVY, 100, 2),
            event(protocol::DIRECTION_YES_HEAVY, 100, 3),
        ];
        let mut gw = gateway(events);
        let outcomes = gw.run(Some(2)).await.unwrap();
        assert_eq!(outcomes.len(), 2, "stopped after 2 events");
    }

    #[tokio::test]
    async fn retune_submits_update_and_adopts_new_params() {
        let mut gw = gateway(vec![]);
        let before = gw.params();

        // 40% vol → wider premium, tighter threshold than the calm canonical defaults.
        let conditions = MarketConditions { implied_vol_bps: 4_000, fair_value: 600_000 };
        let sig = gw.retune(&conditions, &LinearVolPolicy::default()).await.unwrap();

        assert!(!sig.is_empty(), "a signature string is returned");
        let after = gw.params();
        assert_ne!(after, before, "local params adopted the retune target");
        assert_eq!(after.base_oracle_price, 600_000);
        after.validate().unwrap();
    }
}
