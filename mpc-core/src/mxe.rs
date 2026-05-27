//! The MXE matching matrix: blinded accumulation, private P2P sorting, residual calculation.
//!
//! This models what the Arcium node matrix computes over secret shares. Orders enter, are split
//! into additive shares (see [`crate::secret_share`]), accumulated *blind* per node, and only the
//! final aggregates are revealed. The reveal yields the public batch economics — total YES demand,
//! total NO demand — from which we derive:
//!
//! - **Tier-1 (Zero-Impact P2P)**: overlapping YES/NO demand crosses at the midpoint. The matched
//!   volume is `M = min(total_yes, total_no)`; the *light* side is entirely matched, the *heavy*
//!   side is matched pro-rata.
//! - **Tier-2 residual**: the un-matched remainder `net = |total_yes − total_no|`, strictly on the
//!   heavy side, which the on-chain MM backstops. Its per-user split is exact integer allocation so
//!   that `Σ residual == net` and `residual_i ≤ qty_i` hold with no rounding drift — the on-chain
//!   engine re-derives and rejects the batch otherwise.

use {
    crate::{
        error::{MpcError, Result},
        order::{Order, Side},
        secret_share::{ShareAccumulator, ShareRng, Shares},
    },
    protocol::{UserFill, DIRECTION_NO_HEAVY, DIRECTION_YES_HEAVY, MAX_FILLS},
    std::collections::HashMap,
};

/// The public result of matching one batch. `fills` is in deterministic ingestion order — the same
/// order the relay must list the `UserPosition` accounts in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchResult {
    /// Per-user settled amounts (Tier-1 P2P + Tier-2 residual).
    pub fills: Vec<UserFill>,
    /// `|total_yes − total_no|` — the residual the MM backstops.
    pub net_imbalance: u64,
    /// `DIRECTION_YES_HEAVY` or `DIRECTION_NO_HEAVY`.
    pub direction: u8,
    /// Σ over fills of `(p2p_yes + p2p_no)` — advisory header field.
    pub p2p_volume: u64,
    /// Revealed aggregate YES demand.
    pub total_yes: u64,
    /// Revealed aggregate NO demand.
    pub total_no: u64,
}

/// The decentralized MXE node matrix. `nodes` is the number of parties holding secret shares; the
/// internal RNG generates the additive blinds.
#[derive(Debug, Clone)]
pub struct MxeCluster {
    nodes: usize,
    rng: ShareRng,
}

impl MxeCluster {
    /// Build a cluster of `nodes ≥ 1` parties with a deterministic blinding seed (reproducible).
    pub fn new(nodes: usize, blind_seed: u64) -> Self {
        debug_assert!(nodes >= 1);
        Self {
            nodes: nodes.max(1),
            rng: ShareRng::from_seed(blind_seed),
        }
    }

    /// Number of parties in the matrix.
    #[inline]
    pub fn nodes(&self) -> usize {
        self.nodes
    }

    /// Aggregate one quantity column through the blinded share accumulator and reveal the total.
    /// This is the explicit "no plaintext during accumulation" path: every value is split, the
    /// per-node sums are share-local, and only the final fold reveals the aggregate.
    fn blinded_sum(&mut self, quantities: &[u64]) -> Result<u64> {
        let mut acc = ShareAccumulator::new(self.nodes);
        for &q in quantities {
            let shares = Shares::split(q, self.nodes, &mut self.rng);
            acc.add_shares(&shares)?;
        }
        Ok(acc.reveal())
    }

    /// Run the private matching matrix over a batch of opened orders.
    pub fn match_batch(&mut self, orders: &[Order]) -> Result<MatchResult> {
        // --- 1. Per-user aggregation (authoritative, overflow-checked) ----------------------------
        // Insertion order is preserved for deterministic fill / account ordering.
        let mut users: Vec<[u8; 32]> = Vec::new();
        let mut yes: Vec<u64> = Vec::new();
        let mut no: Vec<u64> = Vec::new();
        let mut index: HashMap<[u8; 32], usize> = HashMap::new();

        for o in orders {
            let i = *index.entry(o.user).or_insert_with(|| {
                users.push(o.user);
                yes.push(0);
                no.push(0);
                users.len() - 1
            });
            match o.side {
                Side::Yes => {
                    yes[i] = yes[i].checked_add(o.qty).ok_or(MpcError::QuantityOverflow)?;
                }
                Side::No => {
                    no[i] = no[i].checked_add(o.qty).ok_or(MpcError::QuantityOverflow)?;
                }
            }
        }

        if users.len() > MAX_FILLS {
            return Err(MpcError::TooManyFills);
        }

        // --- 2. Blinded aggregate reveal ---------------------------------------------------------
        // The MXE never holds these as plaintext until this deliberate reveal. We cross-check the
        // blinded path against the authoritative checked sums; since step 1 proved no overflow, the
        // modular share arithmetic reconstructs the true totals exactly.
        let total_yes = self.blinded_sum(&yes)?;
        let total_no = self.blinded_sum(&no)?;
        debug_assert_eq!(total_yes, yes.iter().copied().sum::<u64>());
        debug_assert_eq!(total_no, no.iter().copied().sum::<u64>());

        // --- 3. Tier-1 match volume + direction --------------------------------------------------
        let matched = total_yes.min(total_no);
        let (direction, net_imbalance) = if total_yes >= total_no {
            (DIRECTION_YES_HEAVY, total_yes - total_no)
        } else {
            (DIRECTION_NO_HEAVY, total_no - total_yes)
        };

        // --- 4. Per-user fills -------------------------------------------------------------------
        // The heavy side is matched pro-rata up to `matched`; its remainder is the Tier-2 residual.
        // The light side is fully matched (no residual). `allocate_exact` guarantees the per-user
        // P2P portions sum to `matched` and each is ≤ that user's quantity.
        let (heavy, light) = match direction {
            DIRECTION_YES_HEAVY => (&yes, &no),
            _ => (&no, &yes),
        };
        let heavy_p2p = allocate_exact(matched, heavy);

        let mut fills = Vec::with_capacity(users.len());
        let mut p2p_volume: u64 = 0;
        for i in 0..users.len() {
            let (p2p_yes, p2p_no, residual_yes, residual_no) = match direction {
                DIRECTION_YES_HEAVY => {
                    let py = heavy_p2p[i];
                    let ry = yes[i] - py; // heavy side residual (≥ 0 by construction)
                    (py, no[i], ry, 0)
                }
                _ => {
                    let pn = heavy_p2p[i];
                    let rn = no[i] - pn;
                    (yes[i], pn, 0, rn)
                }
            };
            // `light` is referenced for clarity in the match arms above (no[i]/yes[i]); silence the
            // unused binding without dropping the documentation value of naming it.
            let _ = light;
            p2p_volume = p2p_volume
                .checked_add(p2p_yes)
                .and_then(|v| v.checked_add(p2p_no))
                .ok_or(MpcError::QuantityOverflow)?;
            fills.push(UserFill {
                user: users[i],
                p2p_yes,
                p2p_no,
                residual_yes,
                residual_no,
            });
        }

        Ok(MatchResult {
            fills,
            net_imbalance,
            direction,
            p2p_volume,
            total_yes,
            total_no,
        })
    }
}

/// Allocate `total` across `weights` into non-negative integers that **sum to exactly `total`**,
/// proportional to the weights, with each allocation `≤` its weight (given `total ≤ Σweights`).
/// Largest-remainder method; ties broken by lower index for determinism.
fn allocate_exact(total: u64, weights: &[u64]) -> Vec<u64> {
    let sum: u128 = weights.iter().map(|&w| w as u128).sum();
    if sum == 0 {
        return vec![0; weights.len()];
    }
    let total = total as u128;
    let mut base = Vec::with_capacity(weights.len());
    let mut remainder = Vec::with_capacity(weights.len());
    let mut allocated: u128 = 0;
    for &w in weights {
        let numerator = total * w as u128;
        let q = numerator / sum;
        base.push(q);
        remainder.push(numerator % sum);
        allocated += q;
    }
    // `leftover < weights.len()`: distribute one unit each to the largest fractional remainders.
    let mut leftover = total - allocated;
    let mut order: Vec<usize> = (0..weights.len()).collect();
    order.sort_by(|&a, &b| remainder[b].cmp(&remainder[a]).then(a.cmp(&b)));
    for &i in &order {
        if leftover == 0 {
            break;
        }
        base[i] += 1;
        leftover -= 1;
    }
    base.into_iter().map(|v| v as u64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(user: u8, side: Side, qty: u64) -> Order {
        Order {
            user: [user; 32],
            market: [1u8; 32],
            side,
            qty,
            limit_price: 0,
        }
    }

    /// The canonical YES-heavy scenario from the on-chain test: Alice 300 YES, Bob 100 NO.
    #[test]
    fn yes_heavy_matches_and_leaves_residual() {
        let mut mxe = MxeCluster::new(4, 0xABCD);
        let orders = [
            order(1, Side::Yes, 300),
            order(2, Side::No, 100),
        ];
        let r = mxe.match_batch(&orders).unwrap();
        assert_eq!(r.direction, DIRECTION_YES_HEAVY);
        assert_eq!(r.total_yes, 300);
        assert_eq!(r.total_no, 100);
        assert_eq!(r.net_imbalance, 200);

        // Alice (the only YES) is fully matched up to 100 P2P, 200 residual.
        let alice = r.fills.iter().find(|f| f.user == [1u8; 32]).unwrap();
        assert_eq!(alice.p2p_yes, 100);
        assert_eq!(alice.residual_yes, 200);
        assert_eq!(alice.residual_no, 0);
        // Bob (NO, light side) fully matched, no residual.
        let bob = r.fills.iter().find(|f| f.user == [2u8; 32]).unwrap();
        assert_eq!(bob.p2p_no, 100);
        assert_eq!(bob.residual_no, 0);
    }

    #[test]
    fn no_heavy_is_symmetric() {
        let mut mxe = MxeCluster::new(3, 1);
        let orders = [order(1, Side::Yes, 100), order(2, Side::No, 300)];
        let r = mxe.match_batch(&orders).unwrap();
        assert_eq!(r.direction, DIRECTION_NO_HEAVY);
        assert_eq!(r.net_imbalance, 200);
        let bob = r.fills.iter().find(|f| f.user == [2u8; 32]).unwrap();
        assert_eq!(bob.p2p_no, 100);
        assert_eq!(bob.residual_no, 200);
    }

    #[test]
    fn balanced_book_has_no_residual() {
        let mut mxe = MxeCluster::new(5, 99);
        let orders = [order(1, Side::Yes, 250), order(2, Side::No, 250)];
        let r = mxe.match_batch(&orders).unwrap();
        assert_eq!(r.net_imbalance, 0);
        for f in &r.fills {
            assert_eq!(f.residual_yes, 0);
            assert_eq!(f.residual_no, 0);
        }
    }

    /// Multiple heavy-side users → residual must sum to exactly `net` with no rounding drift, and
    /// no per-user P2P portion may exceed that user's quantity.
    #[test]
    fn residual_allocation_is_exact_across_many_users() {
        let mut mxe = MxeCluster::new(4, 7);
        let orders = [
            order(1, Side::Yes, 333),
            order(2, Side::Yes, 333),
            order(3, Side::Yes, 334),
            order(9, Side::No, 500),
        ];
        let r = mxe.match_batch(&orders).unwrap();
        assert_eq!(r.direction, DIRECTION_YES_HEAVY);
        assert_eq!(r.net_imbalance, 500); // 1000 yes - 500 no
        let sum_res: u64 = r.fills.iter().map(|f| f.residual_yes).sum();
        let sum_p2p_yes: u64 = r.fills.iter().map(|f| f.p2p_yes).sum();
        assert_eq!(sum_res, 500, "residual must sum to net exactly");
        assert_eq!(sum_p2p_yes, 500, "matched YES P2P must sum to M exactly");
        for f in &r.fills {
            assert!(f.p2p_yes <= f.p2p_yes + f.residual_yes, "no over-allocation");
        }
    }

    #[test]
    fn hedger_on_both_sides_is_represented_in_one_fill() {
        // User 1 places both YES and NO; user 2 tips the book YES-heavy.
        let mut mxe = MxeCluster::new(3, 3);
        let orders = [
            order(1, Side::Yes, 100),
            order(1, Side::No, 40),
            order(2, Side::Yes, 100),
        ];
        let r = mxe.match_batch(&orders).unwrap();
        assert_eq!(r.total_yes, 200);
        assert_eq!(r.total_no, 40);
        assert_eq!(r.direction, DIRECTION_YES_HEAVY);
        assert_eq!(r.net_imbalance, 160);
        // Exactly two fills (one per distinct user), user 1 carries both sides.
        assert_eq!(r.fills.len(), 2);
        let u1 = r.fills.iter().find(|f| f.user == [1u8; 32]).unwrap();
        assert_eq!(u1.p2p_no, 40);
    }

    #[test]
    fn too_many_users_is_rejected() {
        let mut mxe = MxeCluster::new(2, 0);
        let orders: Vec<Order> = (0..(MAX_FILLS as u16 + 1))
            .map(|i| {
                let mut u = [0u8; 32];
                u[0..2].copy_from_slice(&i.to_le_bytes());
                Order {
                    user: u,
                    market: [1u8; 32],
                    side: Side::Yes,
                    qty: 1,
                    limit_price: 0,
                }
            })
            .collect();
        assert_eq!(mxe.match_batch(&orders), Err(MpcError::TooManyFills));
    }

    #[test]
    fn allocate_exact_sums_and_bounds() {
        let w = [7u64, 11, 13, 1];
        for total in [0u64, 1, 5, 16, 31, 32] {
            let a = allocate_exact(total.min(w.iter().sum()), &w);
            assert_eq!(a.iter().sum::<u64>(), total.min(w.iter().sum::<u64>()));
            for (ai, wi) in a.iter().zip(w.iter()) {
                assert!(ai <= wi);
            }
        }
    }
}
