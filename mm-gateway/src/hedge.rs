//! The delta-hedging engine.
//!
//! When a batch settles with a residual, the on-chain `submit_batch` forces the MM to take the side
//! *opposite* the heavy flow: a YES-heavy batch leaves the MM holding `net_imbalance` NO contracts
//! (`mm_no += net`), a NO-heavy batch leaves it holding YES. That is directional exposure. To lock
//! the spread fee risk-free, the MM immediately buys the **heavy side** on an external lit venue,
//! offsetting its on-chain position one-for-one.
//!
//! The limit price is the on-chain Tier-2 clearing price the residual buyers paid (`heavy_price`),
//! re-derived here with the exact same fixed-point math as the program's `math.rs`. Filling at or
//! below that price preserves the captured premium as a risk-free spread.

use {
    crate::{
        error::{GatewayError, Result},
        event::SettlementEvent,
        risk::RiskParams,
    },
    protocol::{DIRECTION_NO_HEAVY, DIRECTION_YES_HEAVY, MAX_PRICE, MIN_PRICE, SCALE_FACTOR},
};

/// Which contract side the hedge order buys on the external venue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HedgeSide {
    /// Buy YES (offsets an MM-held NO position from a YES-heavy batch).
    Yes,
    /// Buy NO (offsets an MM-held YES position from a NO-heavy batch).
    No,
}

/// A fully-specified offsetting order to send to an external venue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HedgeOrder {
    /// Market the hedge belongs to (for venue symbol mapping / correlation).
    pub market: [u8; 32],
    /// Batch epoch that triggered this hedge.
    pub epoch: u64,
    /// Side to buy on the venue (the heavy side of the batch).
    pub side: HedgeSide,
    /// Contracts to buy — exactly the residual the MM absorbed.
    pub qty: u64,
    /// Worst price to pay (6-dec fixed point): the on-chain Tier-2 clearing price. Filling ≤ this
    /// preserves the spread.
    pub limit_price: u64,
    /// The per-contract spread fee the MM captured on-chain (the Tier-2 premium) — the edge this
    /// hedge is locking in.
    pub captured_premium: u64,
}

/// The on-chain `math::skew_premium`: `premium = max_skew_premium * min(net*SCALE/threshold, SCALE) / SCALE`.
fn skew_premium(net: u64, max_skew_premium: u64, imbalance_threshold: u64) -> Result<u64> {
    if imbalance_threshold == 0 {
        return Err(GatewayError::InvalidRiskParams);
    }
    let scale = SCALE_FACTOR as u128;
    let ratio = ((net as u128 * scale) / imbalance_threshold as u128).min(scale);
    Ok((max_skew_premium as u128 * ratio / scale) as u64)
}

/// The on-chain `math::clearing_price` (YES price), clamped to `[MIN_PRICE, MAX_PRICE]`.
fn clearing_price(net: u64, direction: u8, params: &RiskParams) -> Result<u64> {
    let premium = skew_premium(net, params.max_skew_premium, params.imbalance_threshold)?;
    let raw = match direction {
        DIRECTION_YES_HEAVY => params.base_oracle_price.saturating_add(premium),
        DIRECTION_NO_HEAVY => params.base_oracle_price.saturating_sub(premium),
        _ => return Err(GatewayError::MalformedEvent),
    };
    Ok(raw.clamp(MIN_PRICE, MAX_PRICE))
}

/// Compute the offsetting hedge for a settlement event, or `None` if there was no residual to hedge.
///
/// The side is the *heavy* side (which the MM is short on-chain and must buy back), the size is the
/// residual, and the limit is the heavy-side clearing price (`YES`: the clearing price itself;
/// `NO`: `SCALE − clearing`, since NO and YES prices are complementary).
pub fn compute_hedge(event: &SettlementEvent, params: &RiskParams) -> Result<Option<HedgeOrder>> {
    if !event.has_residual() {
        return Ok(None);
    }
    let net = event.net_imbalance;
    let clearing = clearing_price(net, event.direction, params)?;
    let premium = skew_premium(net, params.max_skew_premium, params.imbalance_threshold)?;

    // The MM is short the heavy side; it buys the heavy side back. `heavy_price` is what the residual
    // buyers paid for that side on-chain — the exact figure the program uses (`SCALE − clearing` on
    // the NO side because the program prices the heavy NO contract complementarily).
    let (side, heavy_price) = match event.direction {
        DIRECTION_YES_HEAVY => (HedgeSide::Yes, clearing),
        DIRECTION_NO_HEAVY => (HedgeSide::No, SCALE_FACTOR - clearing),
        _ => return Err(GatewayError::MalformedEvent),
    };

    Ok(Some(HedgeOrder {
        market: event.market,
        epoch: event.epoch,
        side,
        qty: net,
        limit_price: heavy_price,
        captured_premium: premium,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(direction: u8, net: u64) -> SettlementEvent {
        SettlementEvent {
            market: [7u8; 32],
            epoch: 1,
            batch_id: 1,
            direction,
            net_imbalance: net,
            p2p_volume: 0,
        }
    }

    // The canonical on-chain params: base $0.50, max premium $0.10, threshold 1000.
    fn params() -> RiskParams {
        RiskParams {
            base_oracle_price: 500_000,
            max_skew_premium: 100_000,
            imbalance_threshold: 1_000,
        }
    }

    #[test]
    fn yes_heavy_hedges_by_buying_yes_at_clearing() {
        // net 200 → premium 20_000, clearing 520_000. Mirrors the on-chain settlement test exactly.
        let h = compute_hedge(&event(DIRECTION_YES_HEAVY, 200), &params())
            .unwrap()
            .unwrap();
        assert_eq!(h.side, HedgeSide::Yes);
        assert_eq!(h.qty, 200);
        assert_eq!(h.limit_price, 520_000);
        assert_eq!(h.captured_premium, 20_000);
    }

    #[test]
    fn no_heavy_hedges_by_buying_no_at_complementary_price() {
        // NO-heavy: YES clearing = base - premium = 480_000 → heavy NO price = 520_000.
        let h = compute_hedge(&event(DIRECTION_NO_HEAVY, 200), &params())
            .unwrap()
            .unwrap();
        assert_eq!(h.side, HedgeSide::No);
        assert_eq!(h.qty, 200);
        assert_eq!(h.limit_price, 520_000);
        assert_eq!(h.captured_premium, 20_000);
    }

    #[test]
    fn no_residual_means_no_hedge() {
        assert_eq!(compute_hedge(&event(DIRECTION_YES_HEAVY, 0), &params()).unwrap(), None);
    }

    #[test]
    fn premium_saturates_and_price_clamps() {
        // Huge imbalance saturates skew_ratio at SCALE → premium = max_skew_premium.
        let big = RiskParams {
            base_oracle_price: 950_000,
            max_skew_premium: 100_000,
            imbalance_threshold: 1,
        };
        let h = compute_hedge(&event(DIRECTION_YES_HEAVY, 10_000), &big).unwrap().unwrap();
        // raw = 950_000 + 100_000 = 1_050_000 → clamped to MAX_PRICE.
        assert_eq!(h.limit_price, MAX_PRICE);
    }

    #[test]
    fn bad_direction_is_rejected() {
        let mut ev = event(9, 100);
        ev.direction = 9;
        assert_eq!(compute_hedge(&ev, &params()), Err(GatewayError::MalformedEvent));
    }
}
