//! Economic and layout constants shared verbatim by the on-chain settlement engine and
//! the off-chain Arcium MPC engine. These are part of the frozen interface: changing any
//! value here is a wire-breaking change and must be versioned across both sides.

/// Fixed-point scale. All prices and ratios are expressed in 6-decimal fixed point, so a
/// price of `1.0` USD == `1_000_000`. Chosen to match USDC's 6 decimals, which lets the
/// collateral math (`qty * price / SCALE_FACTOR`) round-trip into raw token base units
/// without an intermediate rescale.
pub const SCALE_FACTOR: u64 = 1_000_000;

/// Tier-1 peer-to-peer cross price: exactly $0.50. Overlapping YES/NO demand is matched
/// here with zero market-maker impact (a YES buyer and a NO buyer fund each other's payout).
pub const MIDPOINT_PRICE: u64 = 500_000;

/// Hard execution-price floor: $0.01. Tier-2 clearing prices are clamped into
/// `[MIN_PRICE, MAX_PRICE]` so a runaway skew can never settle a contract at 0 or 1.
pub const MIN_PRICE: u64 = 10_000;

/// Hard execution-price ceiling: $0.99.
pub const MAX_PRICE: u64 = 990_000;

/// Frequent-Batch-Auction epoch length in slots (~400ms/slot → ~1.2s cadence).
pub const EPOCH_SLOTS: u64 = 3;

/// Maximum number of MPC committee members an immutable `Committee` account can hold.
pub const MAX_COMMITTEE: usize = 8;

/// Upper bound on `UserFill` records the on-chain settlement loop will process in one batch.
/// Bounds compute + stack usage; a frame exceeding this is rejected (never silently truncated).
pub const MAX_FILLS: usize = 64;

/// `BatchHeader.direction`: residual imbalance is on the YES side (net YES buyers); the
/// market maker is the forced NO counterparty and residual fills add a positive premium.
pub const DIRECTION_YES_HEAVY: u8 = 0;

/// `BatchHeader.direction`: residual imbalance is on the NO side; the premium is subtracted.
pub const DIRECTION_NO_HEAVY: u8 = 1;

/// `MarketState.outcome`: market is open / not yet resolved (the zeroed default).
pub const OUTCOME_UNRESOLVED: u8 = 0;
/// `MarketState.outcome`: the event resolved YES — YES contracts pay $1, NO contracts pay $0.
pub const OUTCOME_YES_WON: u8 = 1;
/// `MarketState.outcome`: the event resolved NO — NO contracts pay $1, YES contracts pay $0.
pub const OUTCOME_NO_WON: u8 = 2;
/// `MarketState.outcome`: the market was voided — every contract (YES and NO) settles at the
/// `MIDPOINT_PRICE` ($0.50). Solvent because `yes_total == no_total`, so total payout = locked.
pub const OUTCOME_INVALID: u8 = 3;

/// `MarketState.status`: trading is open — batches settle here; the market cannot be resolved yet.
pub const STATUS_TRADING: u8 = 0;
/// `MarketState.status`: trading is closed — no further settlement; the market can be resolved.
pub const STATUS_CLOSED: u8 = 1;
