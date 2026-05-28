//! Gateway error type. Hand-rolled (no `thiserror`) to keep the dependency surface minimal, matching
//! the on-chain `ShadowError` and the mpc-core `MpcError` style.

use core::fmt;

/// Every fallible gateway operation returns `Result<_, GatewayError>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatewayError {
    /// A settlement frame/log could not be parsed into a [`crate::event::SettlementEvent`].
    MalformedEvent,
    /// Proposed risk parameters violate the on-chain bounds (`InvalidRiskParams` mirror).
    InvalidRiskParams,
    /// Hedge math overflowed (a residual large enough to overflow `u64` collateral).
    HedgeOverflow,
    /// An external venue rejected or failed the order. Carries the venue's reason.
    Venue(String),
    /// A JSON-RPC / network call failed. Carries a human-readable cause.
    Rpc(String),
    /// The WebSocket event stream errored or closed unexpectedly.
    Stream(String),
}

impl fmt::Display for GatewayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GatewayError::MalformedEvent => f.write_str("malformed settlement event"),
            GatewayError::InvalidRiskParams => f.write_str("risk parameters out of on-chain bounds"),
            GatewayError::HedgeOverflow => f.write_str("hedge sizing overflowed u64"),
            GatewayError::Venue(why) => write!(f, "venue order failed: {why}"),
            GatewayError::Rpc(why) => write!(f, "rpc call failed: {why}"),
            GatewayError::Stream(why) => write!(f, "event stream error: {why}"),
        }
    }
}

impl std::error::Error for GatewayError {}

/// Crate-wide result alias.
pub type Result<T> = core::result::Result<T, GatewayError>;
