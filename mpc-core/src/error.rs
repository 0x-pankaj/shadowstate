//! Engine error type. Hand-rolled (no `thiserror`) to keep the dependency surface minimal, matching
//! the zero-dependency `ShadowError` style of the on-chain crate.

use core::fmt;

/// Every fallible operation in the engine returns `Result<_, MpcError>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MpcError {
    /// A sealed-order buffer was shorter than its declared layout, or its length field lied.
    MalformedSealedOrder,
    /// ChaCha20Poly1305 authentication failed — wrong cluster key, corrupted bytes, or a forgery.
    SealOpenFailed,
    /// The plaintext order inside a seal was not a valid fixed-layout `Order`.
    MalformedOrder,
    /// An order's `market` did not match the epoch's target market.
    MarketMismatch,
    /// A secret-sharing operation was given an empty / mismatched share vector.
    InvalidShareSet,
    /// Integer overflow while accumulating order quantities (a single batch exceeded `u64`).
    QuantityOverflow,
    /// The matched batch produced more than [`protocol::MAX_FILLS`] distinct users.
    TooManyFills,
    /// Threshold signing was asked for more signatures than the committee has members, or for a
    /// member index that does not exist.
    InvalidCommittee,
    /// An RPC / ingestion-source read failed. Carries a human-readable cause.
    Ingestion(String),
    /// A revealed batch slot has no recorded owner in the relayer's `SlotRegistry`.
    SlotOwnerMissing(usize),
    /// The relayer's deterministic re-derivation disagrees with the gateway's revealed aggregates
    /// (direction / net imbalance / totals) — the batch is rejected rather than settled on bad data.
    GatewayDisagreement,
}

impl fmt::Display for MpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MpcError::MalformedSealedOrder => f.write_str("malformed sealed-order buffer"),
            MpcError::SealOpenFailed => f.write_str("sealed-order authentication failed"),
            MpcError::MalformedOrder => f.write_str("malformed plaintext order"),
            MpcError::MarketMismatch => f.write_str("order market does not match epoch market"),
            MpcError::InvalidShareSet => f.write_str("invalid secret-share set"),
            MpcError::QuantityOverflow => f.write_str("order-quantity accumulation overflowed u64"),
            MpcError::TooManyFills => f.write_str("batch exceeds MAX_FILLS distinct users"),
            MpcError::InvalidCommittee => f.write_str("invalid committee size / threshold"),
            MpcError::Ingestion(cause) => write!(f, "ingestion source error: {cause}"),
            MpcError::SlotOwnerMissing(slot) => write!(f, "no recorded owner for batch slot {slot}"),
            MpcError::GatewayDisagreement => {
                f.write_str("relayer re-derivation disagrees with gateway-revealed aggregates")
            }
        }
    }
}

impl std::error::Error for MpcError {}

/// Crate-wide result alias.
pub type Result<T> = core::result::Result<T, MpcError>;
