//! Additive secret sharing over the ring `Z/2^64` — the cryptographic primitive underneath the
//! Arcium MXE's "blinded accumulator matrix".
//!
//! ## Scheme
//!
//! A secret `x ∈ Z/2^64` is split into `n` shares `s_0..s_{n-1}` where `s_0..s_{n-2}` are uniformly
//! random and `s_{n-1} = x − Σ s_i (mod 2^64)`. Then `Σ s_i ≡ x (mod 2^64)`. Any `n−1` shares are
//! jointly uniform and reveal **nothing** about `x` (information-theoretic privacy of additive
//! sharing). Each of the `n` MXE nodes holds exactly one share of every order quantity.
//!
//! ## Homomorphic accumulation (why no plaintext leaks)
//!
//! Addition is share-local: if node `k` holds `s_k(a)` and `s_k(b)`, then `s_k(a)+s_k(b)` is a valid
//! share of `a+b` with no communication and no plaintext. The MXE sums every order's share *inside*
//! each node; only the final aggregate is reconstructed (revealed), and that aggregate — total YES,
//! total NO, the residual — is exactly what becomes public on-chain anyway. Individual order sizes
//! and identities never exist in plaintext during the computation cycle.
//!
//! Arithmetic is wrapping (mod 2^64) on purpose: the blinding terms are full-width random `u64`, so
//! intermediate node shares routinely wrap. Reconstruction wraps back to the true sum. The engine
//! separately guarantees (in [`crate::mxe`]) that the *true* aggregated quantities never exceed
//! `u64`, so the revealed plaintext sums are exact, not modular artifacts.

/// A deterministic SplitMix64 PRNG. Used to generate share-blinding terms. Seeding it explicitly
/// makes the whole MPC run reproducible in tests; production seeds it from a CSPRNG (see
/// [`ShareRng::from_entropy`]).
#[derive(Debug, Clone)]
pub struct ShareRng {
    state: u64,
}

impl ShareRng {
    /// Seed the PRNG explicitly (deterministic, reproducible).
    pub fn from_seed(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Seed from the OS CSPRNG (production path).
    pub fn from_entropy() -> Self {
        use rand::RngCore;
        let mut os = rand::rngs::OsRng;
        Self {
            state: os.next_u64(),
        }
    }

    /// Next 64 uniform bits (SplitMix64 — Steele/Lea/Vigna).
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// `n` additive shares of a single secret value. Invariant: `Σ shares ≡ secret (mod 2^64)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shares(pub Vec<u64>);

impl Shares {
    /// Split `secret` into `n ≥ 1` additive shares using `rng` for the `n−1` random blinds.
    pub fn split(secret: u64, n: usize, rng: &mut ShareRng) -> Self {
        debug_assert!(n >= 1, "need at least one share");
        if n == 1 {
            return Shares(vec![secret]);
        }
        let mut shares = Vec::with_capacity(n);
        let mut acc: u64 = 0;
        for _ in 0..n - 1 {
            let r = rng.next_u64();
            acc = acc.wrapping_add(r);
            shares.push(r);
        }
        // Final share closes the sum to `secret` (mod 2^64).
        shares.push(secret.wrapping_sub(acc));
        Shares(shares)
    }

    /// Number of shares (== number of nodes).
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True if there are no shares.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Reconstruct the secret by wrapping-summing all shares.
    #[inline]
    pub fn reconstruct(&self) -> u64 {
        self.0.iter().fold(0u64, |a, &s| a.wrapping_add(s))
    }
}

/// The per-node blinded accumulator: one running share-sum per node. This is the data structure the
/// MXE node matrix actually holds — node `k` only ever sees column `k`, never a plaintext quantity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareAccumulator {
    node_sums: Vec<u64>,
}

impl ShareAccumulator {
    /// A zeroed accumulator for an `n`-node matrix.
    pub fn new(n: usize) -> Self {
        Self {
            node_sums: vec![0u64; n],
        }
    }

    /// Number of nodes.
    #[inline]
    pub fn nodes(&self) -> usize {
        self.node_sums.len()
    }

    /// Fold one secret's shares into the running per-node totals. Returns an error if the share
    /// vector's width does not match the node count.
    pub fn add_shares(&mut self, shares: &Shares) -> crate::error::Result<()> {
        if shares.len() != self.node_sums.len() || shares.is_empty() {
            return Err(crate::error::MpcError::InvalidShareSet);
        }
        for (slot, &s) in self.node_sums.iter_mut().zip(shares.0.iter()) {
            *slot = slot.wrapping_add(s);
        }
        Ok(())
    }

    /// Reveal the accumulated secret (sum across nodes). This is the single, deliberate
    /// reconstruction step that models the MXE publishing its aggregate output.
    #[inline]
    pub fn reveal(&self) -> u64 {
        self.node_sums.iter().fold(0u64, |a, &s| a.wrapping_add(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_then_reconstruct_roundtrips() {
        let mut rng = ShareRng::from_seed(0xDEAD_BEEF);
        for &secret in &[0u64, 1, 42, 500_000, u64::MAX, 1_000_000_000_000] {
            for n in 1..=8 {
                let shares = Shares::split(secret, n, &mut rng);
                assert_eq!(shares.len(), n);
                assert_eq!(shares.reconstruct(), secret, "n={n} secret={secret}");
            }
        }
    }

    #[test]
    fn any_n_minus_one_shares_are_independent_of_secret() {
        // Same RNG seed + same blinds → only the *last* share differs between two secrets; the
        // first n-1 shares are identical regardless of the secret, i.e. they carry no information.
        let n = 5;
        let a = Shares::split(111, n, &mut ShareRng::from_seed(7));
        let b = Shares::split(999_999, n, &mut ShareRng::from_seed(7));
        assert_eq!(a.0[..n - 1], b.0[..n - 1], "first n-1 shares must not depend on the secret");
        assert_ne!(a.0[n - 1], b.0[n - 1]);
    }

    #[test]
    fn accumulator_sums_homomorphically() {
        let mut rng = ShareRng::from_seed(1);
        let n = 4;
        let values = [10u64, 250, 7, 999_000, 0, 333];
        let mut acc = ShareAccumulator::new(n);
        for &v in &values {
            acc.add_shares(&Shares::split(v, n, &mut rng)).unwrap();
        }
        let expected: u64 = values.iter().sum();
        assert_eq!(acc.reveal(), expected);
    }

    #[test]
    fn accumulator_rejects_wrong_width() {
        let mut acc = ShareAccumulator::new(3);
        let mut rng = ShareRng::from_seed(2);
        let bad = Shares::split(5, 4, &mut rng);
        assert_eq!(acc.add_shares(&bad), Err(crate::error::MpcError::InvalidShareSet));
    }
}
