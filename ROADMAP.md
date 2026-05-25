# ShadowState Roadmap — to a fully confidential prediction-market dark pool

This is the prioritized plan from "tested locally" to a production confidential dark pool. Legend:
🔴 blocks the confidential **MVP** (real privacy, end-to-end, on devnet) · 🟠 needed for a real market
· 🟢 production / post-MVP. "Buildable now" = pure Rust, testable in this repo; "toolchain" = needs the
Arcium Docker environment + a devnet cluster (cannot be compiled/run here).

## Done

- `protocol/` frozen wire contract · `program/` Pinocchio settlement (2-tier clearing, native committee
  verify, **full collateralization + resolution + winner payout**) · `mpc-core/` relayer + MPC *model* ·
  `mm-gateway/` MM relayer (hedge/risk/portal) · `shadowstate_mxe/` the real Arcium 0.10.4 project
  (`encrypted-ixs/` Arcis circuits + Anchor MXE gateway). **110 Rust tests green, 0 build warnings.**
- Honest status: the Arcium circuits **compile** and the gateway is **deployed + MXE-active on devnet**
  (cluster 456); the end-to-end MPC run is pending a non-rate-limited RPC. The single-process `mpc-core`
  remains the offline *model*. Privacy target = **confidential order flow, positions public**.

---

## Phase 1 — Confidential MVP (the critical path) 🔴

The minimal system that actually matches privately on Arcium and settles + resolves on-chain.

| # | Item | Where | Buildable now? |
|---|---|---|---|
| 1.1 | **Relayer** — gateway `BatchCleared` → deterministic pro-rata → `protocol` frame → settle ✅ **done** | `mpc-core` | ✅ done |
| 1.2 | **Client** — seal an order (RescueCipher) → gateway `ingest_order` ✅ **written** | TS (`@arcium-hq/client`) | ⚠️ to-spec (toolchain) |
| 1.3 | `program/` **trusted-gateway authority mode** — `SubmitBatchTrusted` (disc 10), settlement authority signs, committee stays fallback ✅ **done** | `program/` | ✅ done |
| 1.4 | `arcium build` circuits + gateway ✅ **done** (deployed to devnet `E3GF…dNXe`) | `shadowstate_mxe` | ✅ done |
| 1.5 | **Devnet deploy** + init comp-defs + attach cluster (offset 456) | ops | ⚠️ toolchain |
| 1.6 | **TypeScript e2e test** — seal → ingest → clear → relay → settle → resolve → payout, on devnet | TS | ⚠️ toolchain |

Exit criteria: a real order is hidden until batch close, matched in the MXE, and settled + resolved
on-chain, end to end on devnet.

---

## Phase 2 — A real market (mechanics & robustness) 🟠

| # | Item | Status |
|---|---|---|
| 2.1 | **Limit-order matching** in the circuit (`limit_price` is dropped today; enforce non-crossing orders don't fill — costly MPC comparisons) | ⬜ todo (Arcis, toolchain) |
| 2.2 | **Per-user insolvency handling** (one underfunded user aborts the whole batch) — fundamentally an off-chain matcher concern (only match funded orders) | ⬜ todo (relayer) |
| 2.3 | **MM collateral withdrawal** — `WithdrawMmCollateral` (disc 12), reclaims the unreserved float (reserved-floor enforced automatically) | ✅ **done** |
| 2.4 | **Market lifecycle** — `status` (TRADING/CLOSED) + `CloseMarket` (disc 11); settlement gated to TRADING, resolution to CLOSED | ✅ **done** |
| 2.5 | **Optimistic oracle resolution** — replace the single trusted resolver with UMA-style propose/dispute/bond/finalize | ⬜ todo |
| 2.6 | `INVALID`/voided-market refunds — `OUTCOME_INVALID` settles every contract at the $0.50 midpoint (solvent) | ✅ **done** |
| 2.7 | Order lifecycle — cancel / modify / resting (GTC) across epochs / expiry | ⬜ todo |
| 2.8 | Conservation invariant test — full lifecycle drains the vault to exactly zero, no value created/destroyed | ✅ **done** |

---

## Phase 3 — Production & scale 🟢

| # | Item | Notes |
|---|---|---|
| 3.1 | **Circuit scale** | `BATCH_CAP = 8` → 100s–1000s (pagination / `Pack` / multi-book). |
| 3.2 | Protocol fees | Maker/taker/settlement fee capture. |
| 3.3 | MM risk | Margin/liquidation, insurance fund, bad-debt handling. |
| 3.4 | Price oracle | Feed `base_oracle_price` (Tier-2 anchor) instead of manual MM set. |
| 3.5 | `mm-gateway` live adapters | Real Arcium WS log shape, real venue APIs (Polymarket/Kalshi auth+symbols), funded RPC. |
| 3.6 | Relayer ops | Keys, monitoring, liveness, retries; the 1200 ms loop against real RPC + gateway. |
| 3.7 | Indexer + API + frontend | Positions/fills/markets; a usable app. |
| 3.8 | **Security audit** | On-chain program + **Arcis circuit reveal surface** + gateway trust boundary. |
| 3.9 | Key management | Committee/cluster keys, MM authority, resolver. |

---

## Strategic option — *fully* confidential (hidden positions) 🟢

Today's model keeps **final positions + identities public** on-chain (the locked decision). Hiding
positions too needs Token-2022 confidential transfers (ZK ElGamal) or encrypted MXE position state — a
large addition. Note also inherent metadata leakage: batch size and timing are public.

---

## Suggested order

Phase 1 first (1.1 → 1.3 buildable now; 1.4–1.6 when the toolchain/cluster is available), then the
Phase 2 mechanics (limit orders + insolvency + lifecycle + oracle), then Phase 3 hardening.
