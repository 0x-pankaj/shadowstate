//! Risk parameters and the policy that retunes them as volatility shifts.
//!
//! These are exactly the three knobs the on-chain `UpdateRiskParams` instruction writes, with the
//! identical validation bounds (re-checked here so the gateway never builds a transaction the
//! program would reject).

use {
    crate::error::{GatewayError, Result},
    protocol::{MAX_PRICE, MIN_PRICE},
};

/// The on-chain-tunable Tier-2 pricing parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RiskParams {
    /// Tier-2 PropAMM anchor / fair value (6-dec fixed point).
    pub base_oracle_price: u64,
    /// Maximum premium added at full skew (6-dec).
    pub max_skew_premium: u64,
    /// Net imbalance at which `skew_ratio` saturates.
    pub imbalance_threshold: u64,
}

impl RiskParams {
    /// Validate against the exact on-chain bounds (`update_risk_params`):
    /// `imbalance_threshold != 0`, `MIN_PRICE ≤ base_oracle_price ≤ MAX_PRICE`,
    /// `max_skew_premium ≤ MAX_PRICE`.
    pub fn validate(&self) -> Result<()> {
        if self.imbalance_threshold == 0
            || self.base_oracle_price < MIN_PRICE
            || self.base_oracle_price > MAX_PRICE
            || self.max_skew_premium > MAX_PRICE
        {
            return Err(GatewayError::InvalidRiskParams);
        }
        Ok(())
    }
}

/// Live market inputs the policy reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarketConditions {
    /// Implied volatility, in basis points (e.g. `2_000` = 20%).
    pub implied_vol_bps: u32,
    /// Current best estimate of fair value (6-dec fixed point) from the MM's pricing model.
    pub fair_value: u64,
}

/// A retuning policy: given live conditions, produce the target on-chain risk parameters.
pub trait RiskPolicy {
    /// Compute the target parameters. Implementations MUST return params that pass
    /// [`RiskParams::validate`] (clamp internally).
    fn target(&self, conditions: &MarketConditions) -> RiskParams;
}

/// A simple, well-behaved policy: the skew premium scales linearly with implied vol (wider spreads
/// when the market is jumpy), and the imbalance threshold tightens as vol rises (the MM saturates
/// its premium sooner, charging more for the same imbalance). `base_oracle_price` tracks the MM's
/// fair value. Every output is clamped into the on-chain-valid range.
#[derive(Debug, Clone, Copy)]
pub struct LinearVolPolicy {
    /// Premium added per basis point of implied vol (6-dec per bp).
    pub premium_per_bp: u64,
    /// Imbalance threshold at zero vol (loosest).
    pub base_threshold: u64,
    /// How much the threshold tightens per bp of vol (subtracted, floored at `min_threshold`).
    pub threshold_tighten_per_bp: u64,
    /// Floor for the imbalance threshold (must stay ≥ 1).
    pub min_threshold: u64,
}

impl Default for LinearVolPolicy {
    fn default() -> Self {
        // Sensible defaults: at 20% vol (2000 bp) → premium ≈ $0.10, threshold ≈ 1000.
        Self {
            premium_per_bp: 50, // 2000 bp * 50 = 100_000 = $0.10
            base_threshold: 5_000,
            threshold_tighten_per_bp: 2, // 2000 bp * 2 = 4000 → 5000-4000 = 1000
            min_threshold: 1,
        }
    }
}

impl RiskPolicy for LinearVolPolicy {
    fn target(&self, c: &MarketConditions) -> RiskParams {
        let vol = c.implied_vol_bps as u64;

        // Premium scales with vol, clamped to the on-chain ceiling.
        let max_skew_premium = vol.saturating_mul(self.premium_per_bp).min(MAX_PRICE);

        // Threshold tightens with vol, floored so it never hits 0 (which the program rejects).
        let tighten = vol.saturating_mul(self.threshold_tighten_per_bp);
        let imbalance_threshold = self
            .base_threshold
            .saturating_sub(tighten)
            .max(self.min_threshold.max(1));

        // Anchor tracks fair value, clamped into the executable band.
        let base_oracle_price = c.fair_value.clamp(MIN_PRICE, MAX_PRICE);

        RiskParams {
            base_oracle_price,
            max_skew_premium,
            imbalance_threshold,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_matches_on_chain_bounds() {
        assert!(RiskParams { base_oracle_price: 500_000, max_skew_premium: 100_000, imbalance_threshold: 1_000 }
            .validate()
            .is_ok());
        // threshold 0 rejected
        assert!(RiskParams { base_oracle_price: 500_000, max_skew_premium: 0, imbalance_threshold: 0 }
            .validate()
            .is_err());
        // base below floor / above ceiling rejected
        assert!(RiskParams { base_oracle_price: MIN_PRICE - 1, max_skew_premium: 0, imbalance_threshold: 1 }
            .validate()
            .is_err());
        assert!(RiskParams { base_oracle_price: MAX_PRICE + 1, max_skew_premium: 0, imbalance_threshold: 1 }
            .validate()
            .is_err());
        // premium above ceiling rejected
        assert!(RiskParams { base_oracle_price: 500_000, max_skew_premium: MAX_PRICE + 1, imbalance_threshold: 1 }
            .validate()
            .is_err());
    }

    #[test]
    fn policy_produces_valid_params_and_reacts_to_vol() {
        let policy = LinearVolPolicy::default();
        let calm = policy.target(&MarketConditions { implied_vol_bps: 500, fair_value: 600_000 });
        let jumpy = policy.target(&MarketConditions { implied_vol_bps: 4_000, fair_value: 600_000 });

        calm.validate().unwrap();
        jumpy.validate().unwrap();
        assert_eq!(calm.base_oracle_price, 600_000);
        // Higher vol → wider premium, tighter threshold.
        assert!(jumpy.max_skew_premium > calm.max_skew_premium);
        assert!(jumpy.imbalance_threshold < calm.imbalance_threshold);
    }

    #[test]
    fn policy_clamps_extremes_into_valid_range() {
        let policy = LinearVolPolicy::default();
        // Extreme vol would overshoot the premium ceiling and drive the threshold negative.
        let extreme = policy.target(&MarketConditions { implied_vol_bps: 1_000_000, fair_value: 5 });
        extreme.validate().unwrap();
        assert_eq!(extreme.max_skew_premium, MAX_PRICE);
        assert!(extreme.imbalance_threshold >= 1);
        assert_eq!(extreme.base_oracle_price, MIN_PRICE); // fair_value 5 clamped up
    }

    #[test]
    fn default_policy_recovers_canonical_params_at_20pct_vol() {
        let policy = LinearVolPolicy::default();
        let p = policy.target(&MarketConditions { implied_vol_bps: 2_000, fair_value: 500_000 });
        assert_eq!(p.max_skew_premium, 100_000); // $0.10
        assert_eq!(p.imbalance_threshold, 1_000);
        assert_eq!(p.base_oracle_price, 500_000);
    }
}
