# ShadowState Protocol

> **A confidential, on-chain prediction-market dark pool on Solana — sealed order flow, transparent settlement.**

Orders are **encrypted client-side** and matched **privately inside an [Arcium](https://arcium.com) MXE**
over multi-party computation — so no node, relayer, or rival trader can read an order until it fills.
Every ~1.2 seconds a **Frequent Batch Auction** clears in two tiers (peer-to-peer first, a hedged
market-maker backstop second), and a raw **[Pinocchio](https://github.com/anza-xyz/pinocchio)**
`#![no_std]` engine settles collateral in **Token-2022** — zero heap, zero-copy, atomic per batch.

<p>
  <img alt="Solana" src="https://img.shields.io/badge/Solana-devnet-14F195?logo=solana&logoColor=000">
  <img alt="Arcium" src="https://img.shields.io/badge/Arcium-MXE%20live-7C3AED">
  <img alt="Pinocchio" src="https://img.shields.io/badge/Pinocchio-no__std%20%C2%B7%20zero--copy-000">
  <img alt="Rust 2024" src="https://img.shields.io/badge/Rust-2024-CE412B?logo=rust&logoColor=fff">
  <img alt="tests" src="https://img.shields.io/badge/tests-108%20passing-success">
  <img alt="license" src="https://img.shields.io/badge/license-MIT-blue">
</p>

> **No Anchor. No `solana-program`. No heap.** The settlement program is raw Pinocchio with native
> Ed25519-precompile signature verification and `bytemuck::Pod` account state.

---

## Why it matters — four pillars

|  | Pillar | How ShadowState delivers it |
|---|---|---|
| 🔒 | **Confidential** | Orders are sealed with **x25519 + the Rescue cipher** in the browser and matched on **secret shares** inside an Arcium MXE (Cerberus MPC). Nothing is revealed but the cleared aggregate — front-running has no surface to attack. |
| 🌐 | **Decentralized** | Matching runs across **independent Arcium nodes**, not a trusted operator; settlement is a permissionless on-chain program. Trust is **cryptographic**, anchored by the cluster's callback attestation. |
| ⚡ | **Scalable** | Users write to **isolated ingestion PDAs**, never one contended order book, so **Sealevel stays parallel**; matching is amortized off-chain and the whole batch settles in **one transaction**. |
| ⛓️ | **On-chain settlement** | A `no_std`, zero-copy Pinocchio engine verifies the batch, runs deterministic two-tier clearing, and moves collateral via Token-2022 — **deterministic, auditable, atomic.** |

---

## Live on devnet

Both on-chain programs are **deployed and executable on Solana devnet today**:

| Program | Role | Framework | Program ID |
|---|---|---|---|
| **Settlement engine** (`program/`) | Holds collateral, writes positions, pays out | Pinocchio (no-Anchor) | [`FP8riDGv8jrif5G8QfprfzPeNR8kQ3UUrpTp6EXByDVZ`](https://explorer.solana.com/address/FP8riDGv8jrif5G8QfprfzPeNR8kQ3UUrpTp6EXByDVZ?cluster=devnet) |
| **Arcium MXE gateway** (`shadowstate_mxe/`) | Sealed-order ingestion + confidential clearing | Anchor (isolated glue) | [`E3GFUytcsMFgYgwTrHoob1YvhB4UvqTzj4bFWzE5dNXe`](https://explorer.solana.com/address/E3GFUytcsMFgYgwTrHoob1YvhB4UvqTzj4bFWzE5dNXe?cluster=devnet) |

The full **settlement product** (create market → mint test-USDC → deposit → resolve → claim) works on
devnet now via the `web/` dApp. The end-to-end **confidential-matching loop** needs the one-time
on-chain circuit upload + relayer service — gated only on a non-rate-limited RPC (see
[`DEPLOYMENT.md`](DEPLOYMENT.md)).

---

## The market-maker is hedged — not exposed

A natural objection: if a market-maker (MM) **backstops every unmatched order**, doesn't it just
accumulate risk and lose? No. The MM is **fully collateralized on-chain**, and the moment it absorbs a
residual position, the **`mm-gateway` delta-hedges that exposure onto external venues** (Polymarket,
Kalshi) to net it toward zero. The MM earns the **spread**, not a directional bet — which is what makes
a brand-new confidential venue **liquid from day one**.

---

## What this is (and isn't) — read this first

| Layer | Status | Notes |
|---|---|---|
| `program/` — on-chain settlement engine | ✅ **Live on devnet** | Compiles to SBF; settlement, full collateralization, resolution + winner payout, all tested against the real Token-2022 + Ed25519 precompile. |
| `protocol/` — frozen wire contract | ✅ **Real** | The immutable byte interface shared by every crate, with compile-time size asserts. |
| `shadowstate_mxe/` — Arcium project (circuits + gateway) | 🟢 **Built + deployed to devnet** | Genuine confidential computation — `encrypted-ixs/`: `init_book` / `ingest_order` / `clear_batch` — plus the Anchor MXE gateway. Compiled by `arcium build`; gateway live at `E3GF…dNXe`. |
| `mpc-core/` — relayer + MPC model | ⚠️ **Faithful local model** | Re-derives per-user fills from the gateway's `BatchCleared` and builds the exact `protocol` frame the on-chain program accepts byte-for-byte. The single-process model mirrors the MPC primitives; the live decentralized run is the next wiring step. |
| `mm-gateway/` — MM hedging relayer | ⚠️ **Real logic, adapters un-live-tested** | Hedge / risk / portal math is fully tested; the WS/HTTP/RPC venue adapters are real code, not yet run against live Arcium / Polymarket / an RPC node. |

**Everything builds; 108 tests pass; zero compiler/clippy warnings.** The code is correct and
internally consistent end-to-end. Both contracts are **deployed on devnet**; the remaining work is
**operational wiring** (circuit upload → relayer loop → live venue adapters), not new protocol design.

> **Resolution & full collateralization are implemented.** Every contract is backed by `$1` in the
> vault (Tier-1 YES/NO pairs + the MM backstop via `DepositMmCollateral`); `ResolveMarket` sets the
> outcome and `ClaimWinnings` / `ClaimMmWinnings` redeem winners at `$1`/contract. A trusted resolver
> authority sets the outcome today — the hook for an optimistic oracle.

---

## Architecture

ShadowState is a **three-layer system**. Each layer has one job, and the boundaries are where the
trust model changes — confidential → deterministic → on-chain.

| # | Layer | Crate / where | Responsibility | Trust anchor |
|---|---|---|---|---|
| **1** | **Confidential circuit layer** (Arcium MXE) | `shadowstate_mxe/encrypted-ixs/` — 3 Arcis circuits | Encrypts orders, **matches them over MPC secret shares** inside the Arcium MXE, reveals **only** the cleared aggregate. `init_book` opens the encrypted book · `ingest_order` folds in a sealed order · `clear_batch` runs the auction. | Cerberus MPC — no single node sees an order |
| **2** | **Coordination / relayer layer** | `mpc-core/` (+ `mm-gateway/`) | Listens for `BatchCleared`, **re-derives per-user fills** deterministically (two-tier), builds the `protocol` frame, and submits it. `mm-gateway` delta-hedges the MM's residual onto external venues. | Deterministic & verifiable against the revealed clearing |
| **3** | **On-chain settlement layer** (Pinocchio) | `program/` | Verifies the batch authority, runs **Tier-1 P2P + Tier-2 MM** clearing, mutates the zero-copy position ledger, moves **Token-2022** collateral — atomically per batch. | Solana consensus + the gateway/committee attestation |

> **The Arcium circuit layer (Layer 1) is where the privacy lives** — it is the confidential matching
> engine, distinct from the on-chain settlement program. Orders only ever exist in the clear *after*
> Layer 1 hands a result down.

```
            ┌─────────────────────────── off-chain ───────────────────────────┐
 client     │   Arcium MXE  (3 Arcis circuits)            mm-gateway           │
 seals ──────▶  • init_book  — open encrypted book        • WS event ingest    │
 order      │   • ingest_order — fold sealed order        • delta-hedge engine  │
 (x25519 +  │   • clear_batch  — MPC match on shares      • cross-venue orders  │
  Rescue)   │     → reveals ONLY the aggregate            • risk retune portal  │
            │            │ BatchCleared event                       │          │
            └────────────┼──────────────────────────────────────────┼──────────┘
                  mpc-core relayer: re-derive fills        UpdateRiskParams tx
                  → deterministic two-tier frame                     │
                         │ SubmitBatchTrusted                        │
            ┌────────────▼──────────────────────────────────────────▼──────────┐
 on-chain   │  program/ (Pinocchio, no_std, zero-copy)                          │
            │  • verify committee/gateway authority over the batch              │
            │  • Tier-1 P2P cross @ $0.50  • Tier-2 PropAMM residual pricing     │
            │  • mutate Pod position ledger • Token-2022 TransferChecked         │
            └───────────────────────────────────────────────────────────────────┘
```

**Two-tier clearing.** Tier-1 crosses overlapping YES/NO demand at the **$0.50 midpoint** with zero MM
impact. Tier-2 prices the one-sided residual the MM backstops via a **PropAMM skew premium**, clamped
to `[$0.01, $0.99]`; that premium is the MM's spread fee.

**Confidentiality model.** Privacy lives in the off-chain MPC layer — the only place encrypted orders
exist. On-chain settlement is deterministic **plaintext**: a zero-copy position ledger + plaintext
Token-2022 transfers. The hidden surface is the **order book during the auction** (which is what
defeats front-running); final positions are public — *confidential order flow, public settlement.*

---

## Account-by-account: the life of one trade

Scenario: **Alice buys YES 100, Bob buys NO 100** — they cross peer-to-peer at $0.50 (no MM needed) —
then the market resolves **YES** and Alice claims. Two on-chain programs are involved:

- **GW** = the Anchor MXE gateway (`shadowstate_mxe`, devnet `E3GF…dNXe`)
- **ST** = the Pinocchio settlement engine (`program/`, devnet `FP8ri…ByDVZ`)

PDAs: `MarketState=[b"market",authority]` · `Committee=[b"committee",market]` ·
`VaultAuthority=[b"vault",market]` · `UserPosition=[b"pos",market,owner]` (per user) ·
`BatchBook=[b"book",market,epoch]`. The *Vault* and *MM account* are Token-2022 accounts; the vault's
token authority is the `VaultAuthority` PDA.

### Phase 0 — Market + circuits (one-time)

| # | Ix (prog) | Signer | Accounts touched | Effect |
|---|---|---|---|---|
| 0a | `InitializeMarket` (ST) | authority | payer, authority, mint, **Vault**, **MM acct**, **MarketState**(w), **Committee**(w), system | creates market + committee + binds vault/mint |
| 0b | `DepositMmCollateral` (ST) | MM | MM, **MarketState**(w), MM-token(w), **Vault**(w), mint, token-prog | `TransferChecked` MM→Vault; credits `mm_collateral` |
| 0c | `init_*_comp_def` ×3 + upload (GW) | deployer | MXE, comp-def, LUT, arcium-prog, system | registers `init_book` / `ingest_order` / `clear_batch` circuits |

### Phase 1 — Fund + place **sealed** orders

| # | Ix (prog) | Signer | Accounts touched | Effect |
|---|---|---|---|---|
| 1 | `InitUserPosition` (ST) | Alice | payer, Alice, MarketState, **UserPosition(Alice)**(w), system | creates Alice's position PDA |
| 2 | `DepositCollateral` (ST) | Alice | Alice, MarketState, **UserPosition(Alice)**(w), Alice-token(w), **Vault**(w), mint, token-prog | `TransferChecked` Alice→Vault; credits `collateral` |
| 3 | (repeat 1–2 for Bob) | Bob | … **UserPosition(Bob)**, **Vault** … | Bob funded |
| 4 | `init_book` (GW) | relayer | queue accts¹ + **BatchBook**(w, init) | opens an **encrypted** empty book for `(market, epoch)` |
| 5 | `ingest_order` (GW) | Alice² | queue accts¹ + **BatchBook**(w) | Alice's **sealed** YES-100 folded into the encrypted book — side/size hidden |
| 6 | `ingest_order` (GW) | Bob² | queue accts¹ + **BatchBook**(w) | Bob's **sealed** NO-100 folded in |

¹ *queue accts* = `payer, sign_pda, MXE, mempool, exec_pool, computation, comp_def, cluster, fee_pool, clock, system, arcium_program`.
² each order is sealed with the **client's own** x25519 key to the MXE pubkey; the tx can be sent by the user or a relayer.

### Phase 2 — Batch clears (matching, confidential)

| # | Ix (prog) | Signer | Accounts touched | Effect |
|---|---|---|---|---|
| 7 | `clear_batch` (GW) | relayer | queue accts¹ + **BatchBook** | Arcium ARX nodes run the MPC match over secret shares → emit **`BatchCleared`** (`total_yes=100, total_no=100, matched=100, net=0`) — *only the cleared result is revealed* |

### Phase 3 — Settle on-chain (the only thing pushed to the settlement layer)

| # | Ix (prog) | Signer | Accounts touched | Effect |
|---|---|---|---|---|
| 8 | `SubmitBatchTrusted` (ST) | settlement-authority (relayer) | settlement-authority, **MarketState**(w), **UserPosition(Alice)**(w), **UserPosition(Bob)**(w) | relayer (`mpc-core`) re-derives per-user fills from `BatchCleared`, builds the `protocol` frame, submits → ledger: `Alice.yes_qty=100, Bob.no_qty=100`, collateral debited $0.50 each, `last_epoch++` |

*(Committee path instead: `SubmitBatch` adds the **Committee** + **Instructions sysvar** + Ed25519 precompile ixs.)*

### Phase 4 — Resolution + payout

| # | Ix (prog) | Signer | Accounts touched | Effect |
|---|---|---|---|---|
| 9 | `CloseMarket` (ST) | authority | authority, **MarketState**(w) | trading → `CLOSED` (no more settlement) |
| 10 | `ResolveMarket` (ST) | resolver | resolver, **MarketState**(w) | sets `outcome = YES_WON` |
| 11 | `ClaimWinnings` (ST) | Alice | Alice, MarketState, **UserPosition(Alice)**(w), Alice-token(w), **Vault**(w), mint, **VaultAuthority**, token-prog | `TransferChecked` Vault→Alice = `yes_qty × $1` = 100; zeroes the position. (Bob's losing claim → `NothingToClaim`.) |

**Net:** orders **never appear in the clear** until step 7; everything Solana sees before that is an
*encrypted* `BatchBook`. Money only moves at deposit (2/3), the MM backstop reservation (internal at 8),
and payout (11) — all `TransferChecked` against the same **Vault**.

---

## How Arcium fits — what "going live" needs

[Arcium](https://arcium.com) is a decentralized confidential-computing network on Solana. MXEs
(Multi-party eXecution Environments) run a compiled **Arcis** circuit across independent nodes that
compute over cryptographic secret shares, then post an encrypted result + an on-chain callback.

The real Arcium SDK is Anchor-coupled, which conflicts with this project's **No-Anchor** law. The
resolution (see [`ARCIUM_INTEGRATION.md`](ARCIUM_INTEGRATION.md)) is a **two-program split**: a thin
**Anchor gateway** (`shadowstate_mxe/`) owns *only* the Arcium plumbing, while the **value-bearing
Pinocchio settlement engine stays Anchor-free**. To run the full confidential loop you need:

1. The compiled **Arcis circuits uploaded on-chain** to the MXE (one-time; `init_book` / `ingest_order` / `clear_batch`).
2. A **cluster** (devnet offset `456`) + its x25519 pubkey (clients seal to it).
3. A **funded Solana payer** (computations are metered).
4. The **relayer service** — the ~1.2 s `clear_batch` → `SubmitBatchTrusted` loop.

**No secret credentials are hard-coded or required to build and test this repo** — standard Arcium
onboarding, not a ShadowState-specific secret.

---

## Build & test

```bash
# On-chain program → SBF artifact (edition 2024 needs the v1.52 toolchain)
cargo build-sbf --manifest-path program/Cargo.toml --tools-version v1.52

# Workspace (protocol + program) — runs the LiteSVM settlement suite
cargo test

# Off-chain crates are excluded from the workspace; build/test standalone
cargo test --manifest-path mpc-core/Cargo.toml
cargo test --manifest-path mm-gateway/Cargo.toml
```

The off-chain crates are intentionally **excluded from the workspace `members`** so the on-chain
program's pinned dependency lock (litesvm / agave) stays untouched. They share the frozen `protocol/`
crate by path, so the cross-crate interface cannot drift.

| Crate | Tests | Build target |
|---|---|---|
| `protocol/` | 2 | workspace member |
| `program/` | 33 | workspace member + SBF |
| `mpc-core/` | 42 | standalone |
| `mm-gateway/` | 31 | standalone |
| **Total** | **108** | 0 warnings |

---

## Repository layout

```
protocol/        Frozen wire contract: BatchHeader/UserFill, constants, discriminators, PDA seeds.
program/         On-chain Pinocchio settlement engine (state, math, sig, instructions) + LiteSVM tests.
mpc-core/        Off-chain relayer (BatchCleared → settlement) + the local MPC model/tests.
mm-gateway/      MM relayer: event ingest, delta-hedge, cross-venue dispatch, UpdateRiskParams portal.
shadowstate_mxe/ The real Arcium project — encrypted-ixs/ (Arcis circuits) + the Anchor MXE gateway.
                 Built by `arcium build`; gateway deployed + MXE-active on devnet.
client/          TypeScript client: seal an order (RescueCipher) → gateway ingest_order.
web/             Next.js dApp: create market, mint test-USDC, deposit, sealed order, resolve, claim.
```

See [`ARCIUM_INTEGRATION.md`](ARCIUM_INTEGRATION.md) for the live-deployment plan and the
Anchor-vs-Pinocchio decision that "real Arcium" forces, [`DEPLOYMENT.md`](DEPLOYMENT.md) for the
devnet runbook, and [`ROADMAP.md`](ROADMAP.md) for the path to a production dark pool.

---

## Engineering invariants (enforced)

- Rust 2024 edition, workspace resolver 3.
- On-chain: Pinocchio `#![no_std]`, `no_allocator!`, zero-copy `#[repr(C)]` + `bytemuck::Pod`, no Borsh
  on hotspots, explicit `Result<_, ShadowError>` — no `unwrap` / `expect` / `panic!` in settlement paths.
- No Anchor, no `solana-program`, no typosquatted or license-incompatible dependencies in the
  value-bearing path (Anchor is isolated to the one MXE-glue gateway).
- Frozen cross-crate byte contract in `protocol/` with compile-time size asserts.

## License

MIT.
