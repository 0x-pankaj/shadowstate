//! # ShadowState MM gateway (institutional liquidity hub + cross-venue hedging relayer)
//!
//! The market maker is the on-chain Tier-2 backstop: every batch with a one-sided residual forces
//! it to take the *opposite* side of the heavy flow (see the on-chain `submit_batch`). This crate is
//! the MM's control plane around that role:
//!
//! - [`event`] — parse the settlement outcome of each batch (from the on-chain frame or an Arcium
//!   log line) into a [`SettlementEvent`].
//! - [`hedge`] — the **delta-hedging engine**: read the direction + size the MM just absorbed and
//!   compute the exact offsetting [`HedgeOrder`] that locks the spread fee.
//! - [`venue`] — dispatch that order to an external lit venue (Polymarket / Kalshi / sportsbook)
//!   via the [`VenueClient`] trait (real HTTP adapter + a test mock).
//! - [`risk`] — the risk-parameter model + a volatility policy producing on-chain-valid params.
//! - [`portal`] — the **parameter modification portal**: build + sign the on-chain `UpdateRiskParams`
//!   transaction so the authorized MM wallet can retune `max_skew_premium` / `imbalance_threshold`.
//! - [`rpc`] — a minimal Solana JSON-RPC [`ChainSubmitter`] (blockhash + sendTransaction) over
//!   `reqwest`, no heavyweight `solana-client`.
//! - [`stream`] — the [`EventStream`] trait + a `tokio-tungstenite` Arcium-log WS worker + a mock.
//! - [`gateway`] — the [`Gateway`] orchestrator tying the WS stream → hedger → venue, plus the
//!   retune portal.
//!
//! Network adapters live behind traits so the hedging and risk math are fully unit-tested offline;
//! the concrete WS/HTTP/RPC adapters are thin and compile-checked.

pub mod error;
pub mod event;
pub mod gateway;
pub mod hedge;
pub mod portal;
pub mod risk;
pub mod rpc;
pub mod stream;
pub mod venue;

pub use error::GatewayError;
pub use event::SettlementEvent;
pub use gateway::Gateway;
pub use hedge::{compute_hedge, HedgeOrder, HedgeSide};
pub use portal::{market_pda, update_risk_ix, update_risk_tx};
pub use risk::{LinearVolPolicy, MarketConditions, RiskParams, RiskPolicy};
pub use rpc::{ChainSubmitter, MockSubmitter, RpcSubmitter};
pub use stream::{EventStream, MemoryStream};
pub use venue::{MockVenue, VenueClient, VenueOrderAck};
