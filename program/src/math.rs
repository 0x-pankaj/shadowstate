//! Deterministic two-tier clearing math (fixed-point, 6 decimals). All intermediate products use
//! `u128` so a `u64 * u64` can never overflow; results are range-checked back into `u64`.
//!
//! Mathematical model:
//! - **Tier 1 (P2P)** crosses overlapping YES/NO demand at exactly `MIDPOINT_PRICE` ($0.50). A YES
//!   buyer and a NO buyer fully fund each other's payoff, so the market maker is untouched.
//! - **Tier 2 (PropAMM)** prices the one-sided residual the MM must backstop. The premium scales
//!   linearly with the skew ratio and is clamped so the final price stays inside `[$0.01, $0.99]`.

use {
    crate::error::ShadowError,
    protocol::{
        DIRECTION_NO_HEAVY, DIRECTION_YES_HEAVY, MAX_PRICE, MIN_PRICE, SCALE_FACTOR,
    },
};

/// Tier-2 skew premium (6-dec): `max_skew_premium * skew_ratio / SCALE`, where
/// `skew_ratio = min(net_imbalance * SCALE / imbalance_threshold, SCALE)`. Always
/// `0 <= premium <= max_skew_premium`. This is the MM's spread-fee rate per residual contract.
#[inline]
pub fn skew_premium(
    net_imbalance: u64,
    max_skew_premium: u64,
    imbalance_threshold: u64,
) -> Result<u64, ShadowError> {
    if imbalance_threshold == 0 {
        return Err(ShadowError::InvalidRiskParams);
    }
    let scale = SCALE_FACTOR as u128;
    let skew_ratio = ((net_imbalance as u128) * scale / (imbalance_threshold as u128)).min(scale);
    Ok(((max_skew_premium as u128) * skew_ratio / scale) as u64)
}

/// Tier-2 clearing price for the residual imbalance.
///
/// ```text
/// skew_ratio  = min(net_imbalance * SCALE / imbalance_threshold, SCALE)   // ∈ [0, SCALE]
/// premium     = max_skew_premium * skew_ratio / SCALE                     // ∈ [0, max_skew_premium]
/// raw_price   = base_oracle_price ± premium      (+ if YES-heavy, − if NO-heavy)
/// clearing    = clamp(raw_price, MIN_PRICE, MAX_PRICE)                    // $0.01 .. $0.99
/// ```
#[inline]
pub fn clearing_price(
    net_imbalance: u64,
    direction: u8,
    base_oracle_price: u64,
    max_skew_premium: u64,
    imbalance_threshold: u64,
) -> Result<u64, ShadowError> {
    let premium = skew_premium(net_imbalance, max_skew_premium, imbalance_threshold)? as u128;
    let base = base_oracle_price as u128;

    let raw = match direction {
        DIRECTION_YES_HEAVY => base.saturating_add(premium),
        DIRECTION_NO_HEAVY => base.saturating_sub(premium),
        _ => return Err(ShadowError::FrameEconomicsMismatch),
    };

    // Clamp into the hard guardrail band; result provably fits in u64 (<= MAX_PRICE).
    Ok(raw.clamp(MIN_PRICE as u128, MAX_PRICE as u128) as u64)
}

/// Collateral (mint base units) required/credited for `qty` contracts settled at `price`
/// (6-dec fixed point): `qty * price / SCALE_FACTOR`. Errors on the (practically unreachable)
/// case where the result exceeds `u64`.
#[inline]
pub fn collateral_for(qty: u64, price: u64) -> Result<u64, ShadowError> {
    let v = (qty as u128) * (price as u128) / (SCALE_FACTOR as u128);
    u64::try_from(v).map_err(|_| ShadowError::MathOverflow)
}

/// Checked `a + b` returning the program's overflow error.
#[inline]
pub fn add(a: u64, b: u64) -> Result<u64, ShadowError> {
    a.checked_add(b).ok_or(ShadowError::MathOverflow)
}

/// Checked `a - b` returning the program's overflow error (used for collateral debits).
#[inline]
pub fn sub(a: u64, b: u64) -> Result<u64, ShadowError> {
    a.checked_sub(b).ok_or(ShadowError::MathOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::MIDPOINT_PRICE;

    const BASE: u64 = 500_000; // $0.50 anchor for symmetric assertions
    const MAX_PREMIUM: u64 = 200_000; // $0.20 max premium
    const THRESHOLD: u64 = 1_000; // contracts

    #[test]
    fn zero_imbalance_is_base_price() {
        let p = clearing_price(0, DIRECTION_YES_HEAVY, BASE, MAX_PREMIUM, THRESHOLD).unwrap();
        assert_eq!(p, BASE);
    }

    #[test]
    fn skew_ratio_saturates_at_full_premium() {
        // net_imbalance >= threshold → ratio capped at SCALE → premium == max_skew_premium.
        let p = clearing_price(THRESHOLD, DIRECTION_YES_HEAVY, BASE, MAX_PREMIUM, THRESHOLD).unwrap();
        assert_eq!(p, BASE + MAX_PREMIUM);
        let p2 =
            clearing_price(THRESHOLD * 5, DIRECTION_YES_HEAVY, BASE, MAX_PREMIUM, THRESHOLD).unwrap();
        assert_eq!(p2, BASE + MAX_PREMIUM, "premium must not exceed the cap past saturation");
    }

    #[test]
    fn premium_is_linear_below_threshold() {
        // Half the threshold → half the premium.
        let p = clearing_price(THRESHOLD / 2, DIRECTION_YES_HEAVY, BASE, MAX_PREMIUM, THRESHOLD)
            .unwrap();
        assert_eq!(p, BASE + MAX_PREMIUM / 2);
    }

    #[test]
    fn no_heavy_subtracts_premium() {
        let p = clearing_price(THRESHOLD, DIRECTION_NO_HEAVY, BASE, MAX_PREMIUM, THRESHOLD).unwrap();
        assert_eq!(p, BASE - MAX_PREMIUM);
    }

    #[test]
    fn clamps_to_max_price() {
        // base high, full premium pushes above $0.99.
        let p = clearing_price(THRESHOLD, DIRECTION_YES_HEAVY, 950_000, 200_000, THRESHOLD).unwrap();
        assert_eq!(p, MAX_PRICE);
    }

    #[test]
    fn clamps_to_min_price_without_underflow() {
        // base low, full premium subtracted would go negative → clamp to $0.01, no panic.
        let p = clearing_price(THRESHOLD, DIRECTION_NO_HEAVY, 50_000, 200_000, THRESHOLD).unwrap();
        assert_eq!(p, MIN_PRICE);
    }

    #[test]
    fn zero_threshold_errors() {
        assert_eq!(
            clearing_price(10, DIRECTION_YES_HEAVY, BASE, MAX_PREMIUM, 0),
            Err(ShadowError::InvalidRiskParams)
        );
    }

    #[test]
    fn bad_direction_errors() {
        assert_eq!(
            clearing_price(10, 7, BASE, MAX_PREMIUM, THRESHOLD),
            Err(ShadowError::FrameEconomicsMismatch)
        );
    }

    #[test]
    fn collateral_at_midpoint() {
        // 1000 contracts at $0.50 → 500 base units.
        assert_eq!(collateral_for(1_000, MIDPOINT_PRICE).unwrap(), 500);
    }

    #[test]
    fn collateral_large_values_no_overflow() {
        // 1e12 contracts at $0.99 → 0.99e12, fits u64.
        assert_eq!(collateral_for(1_000_000_000_000, MAX_PRICE).unwrap(), 990_000_000_000);
    }
}
