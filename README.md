# ShadowState Protocol

**A confidential prediction-market dark pool on Solana.** Orders are privately aggregated and
matched off-chain inside an Arcium-style MPC layer every ~1.2 s (a Frequent Batch Auction); a
pure-[Pinocchio](https://github.com/anza-xyz/pinocchio) on-chain engine verifies a committee
threshold signature over the batch, runs deterministic two-tier clearing, and settles collateral in
Token-2022 — all with zero heap allocation and zero-copy `bytemuck::Pod` account state.

> **No Anchor. No `solana-program`. No heap.** The on-chain program is raw Pinocchio `#![no_std]`
> with native Ed25519-precompile signature verification.

---

## What this is (and isn't) — read this first

| Layer | Status | Notes |
|---|---|---|
| `program/` — on-chain settlement engine | ✅ **Real & deployable** | Compiles to SBF, 20 LiteSVM/unit tests against the real Token-2022 + Ed25519 precompile. |
| `protocol/` — frozen wire contract | ✅ **Real** | The immutable byte interface shared by all crates. |
| `mpc-core/` — off-chain MPC engine | ⚠️ **Faithful local model** | Implements the *primitive* Arcium runs (additive secret sharing, sealing, private matching) in one process. It produces frames + signatures the real on-chain program accepts byte-for-byte, but it does **not** yet run on Arcium's live decentralized network. See [Arcium integration](#how-arcium-fits--what-going-live-needs). |
| `shadowstate_mxe/` — **real** Arcium project (circuits + MXE gateway) | 🟢 **Built + deployed to devnet** | The genuine confidential computation (`encrypted-ixs/`: `init_book`/`ingest_order`/`clear_batch`) + the Anchor MXE gateway program. Compiled by `arcium build`; gateway live at `E3GF…dNXe`. See [`ARCIUM_INTEGRATION.md`](ARCIUM_INTEGRATION.md) + [`DEPLOYMENT.md`](DEPLOYMENT.md). |
| `mm-gateway/` — MM hedging relayer | ⚠️ **Real logic, adapters un-live-tested** | Hedging/risk/portal math is fully tested; the WebSocket/HTTP/RPC adapters are real code but have not been run against live Arcium / Polymarket / an RPC node. |

**Everything builds, every test passes (92 total), zero compiler/clippy warnings.** That means the
code is correct and internally consistent end-to-end. It does **not** mean it is deployed to a
cluster or wired to live third-party services — that is the next phase, scoped below.

### Is it a real Polymarket-like market yet?

It is a complete **confidential matching + fully-collateralized settlement core** with P2P crossing,
a market-maker backstop, and **resolution + winner payout** — the hard, novel part. To become a full
production prediction market it still needs:

- **Live Arcium** — the real circuits + MXE gateway (`shadowstate_mxe/`) are built and the gateway is
  deployed + MXE-active on devnet; the end-to-end MPC run needs a non-rate-limited RPC (see below).
- **On-chain sealed-order ingestion.** With real Arcium this is the gateway's `ingest_order`.
- `INVALID`/cancelled-market refunds (today only `YES_WON` / `NO_WON` payout is built).
- **Order lifecycle** — cancellation, cross-epoch resting orders.
- **Operational glue** — an optimistic-oracle resolution source, an indexer, an API, a frontend.

> **Resolution & full collateralization are now implemented.** Every contract is backed by `$1` in
> the vault (Tier-1 YES/NO pairs + the MM backstop posted via `DepositMmCollateral`); `ResolveMarket`
> sets the outcome and `ClaimWinnings`/`ClaimMmWinnings` redeem winners for `$1`/contract. A trusted
> resolver authority sets the outcome today — the hook for an oracle.

---

## Architecture

```
            ┌─────────────────────────── off-chain ───────────────────────────┐
 client     │   mpc-core (Arcium MXE model)              mm-gateway            │
 seals ──────▶  • unseal (x25519+ChaCha20)               • WS event ingest     │
 order      │   • additive secret-share accumulate       • delta-hedge engine  │
            │   • private P2P matching                   • cross-venue orders   │
            │   • residual / direction                   • risk retune portal   │
            │   • threshold Ed25519 sign  ──┐            └──────────┬───────────┘
            └──────────────────────────────┼───────────────────────┼───────────┘
                                  signed batch frame      UpdateRiskParams tx
                                           │                        │
            ┌──────────────────────────────▼────────────────────────▼───────────┐
 on-chain   │  program/ (Pinocchio, no_std, zero-copy)                           │
            │  • verify committee threshold sig (Ed25519 precompile + sysvar)    │
            │  • Tier-1 P2P cross @ $0.50  • Tier-2 PropAMM residual pricing      │
            │  • mutate Pod position ledger • Token-2022 TransferChecked          │
            └───────────────────────────────────────────────────────────────────┘
```

**Two-tier clearing.** Tier-1 crosses overlapping YES/NO demand at the $0.50 midpoint with zero MM
impact. Tier-2 prices the one-sided residual the MM backstops via a PropAMM skew premium, clamped to
`[$0.01, $0.99]`; that premium is paid to the MM as a spread fee.

**Confidentiality model.** Privacy lives entirely in the off-chain MPC layer (the only party that
sees encrypted orders). On-chain settlement is deterministic and *plaintext* — a custom zero-copy
position ledger + plaintext Token-2022 transfers. There is no Token-2022 confidential-transfer
extension; confidentiality is achieved before settlement, in matching/price-discovery.

---

## Account-by-account: the life of one trade

Scenario: **Alice buys YES 100, Bob buys NO 100** — they cross peer-to-peer at $0.50 (no MM needed) —
then the market resolves **YES** and Alice claims. Two on-chain programs are involved:

- **GW** = the Anchor MXE gateway (`shadowstate_mxe`, devnet `E3GF…dNXe`)
- **ST** = the Pinocchio settlement engine (`program/`)

PDAs: `MarketState=[b"market",authority]` · `Committee=[b"committee",market]` ·
`VaultAuthority=[b"vault",market]` · `UserPosition=[b"pos",market,owner]` (per user) ·
`BatchBook=[b"book",market,epoch]`. The *Vault* and *MM account* are Token‑2022 accounts; the vault's
token authority is the `VaultAuthority` PDA.

### Phase 0 — Market + circuits (one-time)

| # | Ix (prog) | Signer | Accounts touched | Effect |
|---|---|---|---|---|
| 0a | `InitializeMarket` (ST) | authority | payer, authority, mint, **Vault**, **MM acct**, **MarketState**(w), **Committee**(w), system | creates market + committee + binds vault/mint |
| 0b | `DepositMmCollateral` (ST) | MM | MM, **MarketState**(w), MM‑token(w), **Vault**(w), mint, token‑prog | `TransferChecked` MM→Vault; credits `mm_collateral` |
| 0c | `init_*_comp_def` ×3 + upload (GW) | deployer | MXE, comp‑def, LUT, arcium‑prog, system | registers `init_book`/`ingest_order`/`clear_batch` circuits |

### Phase 1 — Fund + place **sealed** orders

| # | Ix (prog) | Signer | Accounts touched | Effect |
|---|---|---|---|---|
| 1 | `InitUserPosition` (ST) | Alice | payer, Alice, MarketState, **UserPosition(Alice)**(w), system | creates Alice's position PDA |
| 2 | `DepositCollateral` (ST) | Alice | Alice, MarketState, **UserPosition(Alice)**(w), Alice‑token(w), **Vault**(w), mint, token‑prog | `TransferChecked` Alice→Vault; credits `collateral` |
| 3 | (repeat 1–2 for Bob) | Bob | … **UserPosition(Bob)**, **Vault** … | Bob funded |
| 4 | `init_book` (GW) | relayer | queue accts¹ + **BatchBook**(w, init) | opens an **encrypted** empty book for `(market, epoch)` |
| 5 | `ingest_order` (GW) | Alice² | queue accts¹ + **BatchBook**(w) | Alice's **sealed** YES‑100 folded into the encrypted book — side/size hidden |
| 6 | `ingest_order` (GW) | Bob² | queue accts¹ + **BatchBook**(w) | Bob's **sealed** NO‑100 folded in |

¹ *queue accts* = `payer, sign_pda, MXE, mempool, exec_pool, computation, comp_def, cluster, fee_pool, clock, system, arcium_program`.
² each order is sealed with the **client's own** x25519 key to the MXE pubkey; the tx can be sent by the user or a relayer.

### Phase 2 — Batch clears (matching, confidential)

| # | Ix (prog) | Signer | Accounts touched | Effect |
|---|---|---|---|---|
| 7 | `clear_batch` (GW) | relayer | queue accts¹ + **BatchBook** | Arcium ARX nodes run the MPC match over secret shares → emit **`BatchCleared`** event (`total_yes=100, total_no=100, matched=100, net=0`) — *only the cleared result is revealed* |

### Phase 3 — Settle on-chain (the only thing pushed to the settlement layer)

| # | Ix (prog) | Signer | Accounts touched | Effect |
|---|---|---|---|---|
| 8 | `SubmitBatchTrusted` (ST) | settlement‑authority (relayer) | settlement‑authority, **MarketState**(w), **UserPosition(Alice)**(w), **UserPosition(Bob)**(w) | relayer (`mpc-core`) re‑derives the per‑user fills from `BatchCleared`, builds the `protocol` frame, submits → ledger: `Alice.yes_qty=100, Bob.no_qty=100`, collateral debited $0.50 each, `last_epoch++` |

*(Committee path instead: `SubmitBatch` adds the **Committee** + **Instructions sysvar** + Ed25519 precompile ixs.)*

### Phase 4 — Resolution + payout

| # | Ix (prog) | Signer | Accounts touched | Effect |
|---|---|---|---|---|
| 9 | `CloseMarket` (ST) | authority | authority, **MarketState**(w) | trading → `CLOSED` (no more settlement) |
| 10 | `ResolveMarket` (ST) | resolver | resolver, **MarketState**(w) | sets `outcome = YES_WON` |
| 11 | `ClaimWinnings` (ST) | Alice | Alice, MarketState, **UserPosition(Alice)**(w), Alice‑token(w), **Vault**(w), mint, **VaultAuthority**, token‑prog | `TransferChecked` Vault→Alice = `yes_qty × $1` = 100; zeroes the position. (Bob's YES‑losing claim → `NothingToClaim`.) |

**Net:** the **orders never appear in the clear** until step 7; everything Solana sees before that is an
*encrypted* `BatchBook`. Money only moves at deposit (2/3), the MM backstop reservation (internal at 8),
and payout (11) — all `TransferChecked` against the same **Vault**.

---

## How Arcium fits — what "going live" needs

[Arcium](https://arcium.com) is a decentralized confidential-computing network on Solana. Programs
("MXEs" — Multi-party eXecution Environments) run a compiled **Arcis** circuit across independent
nodes that compute over cryptographic secret shares, then post an encrypted result + an on-chain
callback.

**Do you need credentials?** Not a login — but to run on the real network you do need to onboard:

1. **Write & compile an Arcis circuit** (the matching logic in `mpc-core::mxe` expressed in Arcium's
   DSL) using the `arcium` CLI / `arcis` compiler.
2. **Deploy a computation definition (MXE)** on-chain that references the Arcium program.
3. **A cluster.** On devnet/testnet Arcium provides shared clusters; you configure a *cluster offset*
   and obtain the cluster's x25519 public key (clients seal inputs to it). Mainnet requires a
   provisioned cluster.
4. **A funded Solana payer keypair** to pay for each computation (compute is metered).
5. **Wiring.** The official path uses `arcium-client` — but that crate's default feature pulls in
   Anchor and it is GPL-3.0, both of which violate this project's constraints. **So ShadowState
   verifies the MPC committee natively** (Ed25519 precompile + Instructions sysvar): the on-chain
   `Committee` account stores the *cluster's signing keys*, and the program requires a threshold of
   their signatures over the exact batch bytes. To go live, you populate that committee with the real
   Arcium cluster keys (or your own attested node set) and have `mpc-core` emit/sign frames from the
   cluster's computation output instead of the local model.

In short: **no secret credentials are hard-coded or required to build and test this repo.** Real
Arcium usage requires their CLI + a circuit deployment + a cluster config + a funded payer — standard
Arcium onboarding, not a ShadowState-specific secret.

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
program's carefully pinned dependency lock (litesvm / agave) stays untouched. They share the frozen
`protocol/` crate by path, so the cross-crate interface cannot drift.

| Crate | Tests | Build target |
|---|---|---|
| `protocol/` | 2 | workspace member |
| `program/` | 20 | workspace member + SBF |
| `mpc-core/` | 39 | standalone |
| `mm-gateway/` | 31 | standalone |

---

## Repository layout

```
protocol/        Frozen wire contract: BatchHeader/UserFill, constants, discriminators, PDA seeds.
program/         On-chain Pinocchio settlement engine (state, math, sig, instructions) + LiteSVM tests.
mpc-core/        Off-chain relayer (BatchCleared → settlement) + the local MPC model/tests.
mm-gateway/      MM relayer: event ingest, delta-hedge, cross-venue dispatch, UpdateRiskParams portal.
shadowstate_mxe/ The real Arcium 0.10.4 project — `encrypted-ixs/` (Arcis circuits) + the Anchor MXE
                 gateway program. Built by `arcium build`; gateway deployed + MXE-active on devnet.
client/          TypeScript client: seal an order (RescueCipher) → gateway `ingest_order`.
```

See [`ARCIUM_INTEGRATION.md`](ARCIUM_INTEGRATION.md) for the live-deployment plan and the
Anchor-vs-Pinocchio decision that "real Arcium" forces.

Each crate has its own `README.md` with the detailed design and engineering rules.

---

## Engineering invariants (enforced)

- Rust 2024 edition, workspace resolver 3.
- On-chain: Pinocchio `#![no_std]`, `no_allocator!`, zero-copy `#[repr(C)]` + `bytemuck::Pod`, no
  Borsh on hotspots, explicit `Result<_, ShadowError>` — no `unwrap`/`expect`/`panic!` in settlement
  paths.
- No Anchor, no `solana-program`, no typosquatted or license-incompatible dependencies anywhere.
- Frozen cross-crate byte contract in `protocol/` with compile-time size asserts.

## License

MIT.
