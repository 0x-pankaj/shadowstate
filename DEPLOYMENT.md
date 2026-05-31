# ShadowState — devnet deployment runbook

The operational path from "108 tests green locally" to a **live confidential market on devnet**, where
the core flow actually happens:

> A user places an order. It's confidential. If another user has the opposite side, they're matched
> peer-to-peer; otherwise the market maker fills the remainder — **without revealing any order to the
> world** until the batch clears.

Both on-chain programs are **already deployed to devnet**. What remains is the one-time **circuit
upload** and the **relayer service** that drives the confidential loop. The Arcium toolchain steps
require Docker + the `arcium` CLI + a funded devnet keypair.

---

## Deployed on devnet

| Program | Role | ID |
|---|---|---|
| **Settlement engine** (`program/`, Pinocchio) | Collateral, positions, payout | `FP8riDGv8jrif5G8QfprfzPeNR8kQ3UUrpTp6EXByDVZ` |
| **Arcium MXE gateway** (`shadowstate_mxe/`, Anchor) | Sealed ingestion + confidential clearing | `E3GFUytcsMFgYgwTrHoob1YvhB4UvqTzj4bFWzE5dNXe` |

Cluster: devnet offset **456**. Arcium CLI: **0.10.4**.

---

## What the confidential flow maps to (already built)

| Step | Component | Status |
|---|---|---|
| User deposits test-USDC collateral | `program/` `InitUserPosition` + `DepositCollateral` | ✅ on-chain, tested |
| User places a **sealed** order (side+size hidden) | `client/` → gateway `ingest_order` → MXE encrypted book | ✅ built |
| Orders accumulate **encrypted** (nobody sees them) | `shadowstate_mxe/encrypted-ixs/ingest_order` (Arcis, Cerberus MPC) | ✅ compiled |
| Batch closes (~1.2 s) → P2P match, else MM backstop | `shadowstate_mxe/encrypted-ixs/clear_batch` reveals only the cleared result | ✅ compiled |
| Relayer settles on-chain | `mpc-core::relayer` → `SubmitBatchTrusted` | ✅ lib tested; **service not yet built** |
| MM backstops the residual, fully collateralized | `DepositMmCollateral` + reservation in `submit_batch` | ✅ tested |
| Close → resolve → users claim $1/contract | `CloseMarket` / `ResolveMarket` / `ClaimWinnings` | ✅ tested |

**The confidentiality is real on devnet:** Arcium's ARX nodes run the matching over secret shares — no
single node (or the relayer, or other traders) sees an order until `clear_batch`. After clearing, fills
are public on-chain (the "positions public" model) — the hidden part is the *order book during the
auction*, which is what defeats front-running.

---

## Prerequisites

1. **A Linux/macOS machine with Docker** — the Arcium toolchain runs inside its Docker image. (This is
   the one thing a no-Docker sandbox cannot do.)
2. **Arcium devnet access** — Arcium's devnet is early/permissioned. Confirm the current CLI version,
   the devnet cluster offset (this project targets **0.10.4 / offset 456**), and whether access must be
   requested. Pin the manifests to whatever version the cluster reports.
3. **A funded Solana devnet keypair** — `solana-keygen new` + `solana airdrop 5 --url devnet`.
4. **A non-rate-limited devnet RPC** — the one-time circuit upload sends a burst of transactions that a
   free/public RPC will throttle. Use a paid endpoint (Helius/Triton/QuickNode) **or** the Arcium
   localnet Docker (no devnet RPC at all). **This is the current blocker for the end-to-end loop.**

---

## Phase A — Arcium toolchain (Docker)

Build the Arcium dev container (Ubuntu 24.04 + Rust 1.89 + Solana 2.3.0 + Anchor 0.32.1 + `arcium`
0.10.4), then:

```bash
docker run -d --name shadowstate-dev \
  --ulimit nofile=1048576:1048576 \
  -v "$(pwd)":/app -v "$HOME/.config/solana":/root/.config/solana \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -p 8899:8899 -p 8900:8900 shadowstate-arcium-dev sleep infinity
```

## Phase B — compile ✅ done

The Arcium project lives at **`shadowstate_mxe/`** (created via `arcium init`, circuits in
`encrypted-ixs/`, gateway in `programs/shadowstate_mxe/`) and builds clean:

```bash
cd shadowstate_mxe && arcium build   # → build/*.arcis + target/idl/shadowstate_mxe.json + the .so
```

In 0.10.4 the circuits are uploaded **on-chain** (no separate circuits repo) — done once via the TS
`uploadCircuit` helper.

## Phase C — deploy the MXE gateway ✅ done

```bash
docker exec shadowstate-dev bash -c '
  cd /app/shadowstate_mxe &&
  arcium deploy --cluster-offset 456 --recovery-set-size 4 \
    --rpc-url devnet --keypair-path /root/.config/solana/id.json \
    --program-name shadowstate_arcium_gateway \
    --program-keypair target/deploy/shadowstate_arcium_gateway-keypair.json
'
```
Deployed gateway: **`E3GFUytcsMFgYgwTrHoob1YvhB4UvqTzj4bFWzE5dNXe`** (MXE active, cluster 456).

## Phase D — deploy the settlement program ✅ done

```bash
cargo build-sbf --manifest-path program/Cargo.toml --tools-version v1.52
solana program deploy target/deploy/shadowstate_program.so --url devnet
```
Deployed settlement: **`FP8riDGv8jrif5G8QfprfzPeNR8kQ3UUrpTp6EXByDVZ`**. The program ID is wired into
`web/lib/constants.ts` and the relayer config.

## Phase E — upload circuits + open the book ⏳ blocked on RPC

The remaining one-time job. Against a **non-rate-limited RPC**:

```bash
# 1. Register the 3 computation definitions (init_book / ingest_order / clear_batch)
# 2. uploadCircuit for each .arcis blob (this is the burst the free RPC throttles)
# 3. init_book — open the first encrypted batch book for (market, epoch)
```

## Phase F — run the relayer service ⏳ to build

A small always-on worker that every ~1.2 s triggers the gateway's `clear_batch`, watches for the
`BatchCleared` event, runs `mpc-core::relayer::clear()`, and submits `SubmitBatchTrusted`. It exposes
**no URL** — it needs only outbound RPC + the settlement-authority keypair + the two program IDs.
(Triggering Arcium computations is TypeScript via `@arcium-hq/client`; the deterministic settlement is
the Rust `mpc-core`.)

## Phase G — end-to-end flow test

A TypeScript e2e (`@arcium-hq/client` + the `client/` SDK) proving the whole thing on devnet:

1. Alice deposits, places a **sealed** `BUY YES 100` → hidden in the MXE.
2. Bob deposits, places a **sealed** `BUY NO 100` → hidden.
3. Batch clears → they **match P2P at $0.50** (MM untouched). Assert: nobody could read the orders
   before clearing; positions update after.
4. Carol places `BUY YES 200` with no NO counterparty → **MM backstops** the residual.
5. Close → resolve → winners claim $1/contract. Assert balances.

---

## Remaining build work

| Piece | Notes |
|---|---|
| **Circuit upload + comp-def init** | one-time; blocked only on a non-rate-limited RPC |
| **Relayer service** (clear-trigger + settle loop) | TS trigger + Rust settle; not yet built into a runnable binary |
| TypeScript **e2e flow test** | the Phase-G proof |
| Circuit **scale** (`BATCH_CAP 8 → N`) | before real volume |
| Optimistic-oracle resolution | replace the trusted resolver for trustless resolution |
| `mm-gateway` live venue adapters | real Polymarket/Kalshi auth + symbols |

**Nothing here is blocked on more design** — it's upload + wire + run. The blockers are environmental
(Docker/devnet/Arcium + a paid RPC), not architectural.
