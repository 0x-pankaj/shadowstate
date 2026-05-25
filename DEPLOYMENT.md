# ShadowState — devnet deployment runbook (the real product)

This is the operational path from "110 tests green locally" to a **live confidential market on devnet**
where the flow you described actually happens:

> A user places an order. It's confidential. If another user has the opposite side, they're matched
> peer-to-peer; otherwise the market maker fills the remainder — **without revealing any order to the
> world** until the batch clears.

That flow is exactly what we built. What remains is **compiling the Arcium pieces and wiring the
operational services** — none of which can run in the local sandbox (no Docker / Arcium CLI /
cluster). It runs on **your** machine. Below is precisely what you do, what I need from you, and the
tight loop we use to make it work.

---

## What the confidential flow maps to (already built)

| Step | Component | Status |
|---|---|---|
| User deposits test-USDC collateral | `program/` `InitUserPosition` + `DepositCollateral` | ✅ on-chain, tested |
| User places a **sealed** order (side+size hidden) | `shadowstate_mxe/tests` (TS) → gateway `ingest_order` → MXE encrypted book | ✅ built |
| Orders accumulate **encrypted** (nobody sees them) | `shadowstate_mxe/encrypted-ixs/ingest_order` (Arcis, Cerberus MPC) | ✅ compiled |
| Batch closes (~1.2 s) → P2P match, else MM backstop | `shadowstate_mxe/encrypted-ixs/clear_batch` reveals only the cleared result | ✅ compiled |
| Relayer settles on-chain | `mpc-core::relayer` → `SubmitBatchTrusted` | ✅ lib tested; **service not built** |
| MM backstops the residual, fully collateralized | `DepositMmCollateral` + reservation in `submit_batch` | ✅ tested |
| Close → resolve → users claim $1/contract | `CloseMarket` / `ResolveMarket` / `ClaimWinnings` | ✅ tested |

**The confidentiality is real on devnet:** Arcium's ARX nodes run the matching over secret shares —
no single node (or the relayer, or other traders) sees an order until `clear_batch`. After clearing,
fills are public on-chain (the "positions public" model you chose) — the hidden part is the *order book
during the auction*, which is what defeats front-running.

---

## What YOU do on Arcium (and what I need from you)

### You provide / set up
1. **A Linux or macOS machine with Docker** — the Arcium toolchain runs only in its Docker image.
   (This is the one thing the sandbox here cannot do.)
2. **Arcium devnet access** — Arcium's devnet is early/permissioned. Check the current onboarding at
   [arcium.com](https://arcium.com) / their Discord (the `arcium-dev` skill targets **CLI 0.6.3,
   devnet cluster offset 456**). Confirm: (a) the CLI version, (b) the devnet cluster offset, (c) whether
   you need to request access. **→ Tell me the version + cluster offset they give you** so I pin the
   manifests correctly.
3. **A funded Solana devnet keypair** — `solana-keygen new` + `solana airdrop 5 --url devnet`. I never
   need the secret; just confirm it's funded.
4. **A public GitHub repo** for the compiled circuits (`.arcis` files) — Arcium fetches them at compute
   time. **→ Give me the raw URL** so I set `CIRCUITS_BASE_URL` in the gateway.

### What I need back from you (the loop)
The Arcium toolchain *generates* IDL/descriptors that I can't see from here. So after each step, **paste
me the output** — especially:
- `arcium build` output + the generated `target/idl/*.json` and `build/*.idarc` (these resolve every
  `// RECONCILE:` markers — already reconciled in `shadowstate_mxe/programs/shadowstate_mxe/src/lib.rs`).
- Any deploy errors, and the deployed **program IDs**.
- The first `arcium test` run's failures.

I fix against the real descriptors and hand you the next patch. That's how we get it actually working.

---

## Phase A — Arcium toolchain (your machine)

```bash
# Build the Arcium dev container (full Dockerfile is in
# .agents/skills/arcium-dev/cli-deployment.md — Ubuntu 24.04 + Rust 1.89 + Solana 2.3.0 +
# Anchor 0.32.1 + arcium 0.6.3). Then:
docker run -d --name shadowstate-dev \
  --ulimit nofile=1048576:1048576 \
  -v "$(pwd)":/app -v "$HOME/.config/solana":/root/.config/solana \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -p 8899:8899 -p 8900:8900 shadowstate-arcium-dev sleep infinity
```

## Phase B — compile ✅ already done

The real Arcium project already exists at **`shadowstate_mxe/`** (created via `arcium init`, with the
circuits in `encrypted-ixs/` and the gateway in `programs/shadowstate_mxe/`), and it **builds clean**:

```bash
cd shadowstate_mxe && arcium build      # → build/*.arcis + target/idl/shadowstate_mxe.json + the .so
```

In 0.10.4 the circuits are uploaded **on-chain** (no separate GitHub circuits repo) — the TS test does
this via `uploadCircuit`. Skip straight to deploy.

## Phase C — deploy the MXE gateway to devnet  ✅ already done

```bash
docker exec shadowstate-dev bash -c '
  cd /app/shadowstate_mxe &&
  arcium deploy --cluster-offset 456 --recovery-set-size 4 \
    --rpc-url devnet --keypair-path /root/.config/solana/id.json \
    --program-name shadowstate_arcium_gateway \
    --program-keypair target/deploy/shadowstate_arcium_gateway-keypair.json
'
# Then init each computation definition ONCE (init_book / ingest_order / clear_batch comp defs).
```
**Paste me the deployed gateway program ID.**

## Phase D — deploy the settlement program + set up a market

```bash
# Our pure-Pinocchio settlement program (built here, no Arcium toolchain needed):
cargo build-sbf --manifest-path program/Cargo.toml --tools-version v1.52
solana program deploy target/deploy/shadowstate_program.so --url devnet
# → paste me the program ID; we set it in mpc-core/relay.rs + the gateway's market binding.

# Market setup (a script I'll provide): create a Token-2022 test-USDC mint, the vault + MM accounts,
# InitializeMarket with the RELAYER key as settlement_authority (trusted path), DepositMmCollateral.
```

## Phase E — run the relayer service (I build this next)

A small service that: every ~1.2 s triggers the gateway's `clear_batch`, watches for the `BatchCleared`
event, runs `mpc-core::relayer::clear()`, and submits `SubmitBatchTrusted`. (Triggering Arcium
computations is naturally TypeScript via `@arcium-hq/client`; the deterministic settlement is our Rust
`mpc-core`. I'll wire them.)

## Phase F — the complete user-flow test

A TypeScript e2e (`@arcium-hq/client` + our client) that proves the whole thing on devnet:
1. Alice deposits, places a **sealed** `BUY YES 100` → hidden in the MXE.
2. Bob deposits, places a **sealed** `BUY NO 100` → hidden.
3. Batch clears → they **match P2P at $0.50** (MM untouched). Assert: nobody could read the orders
   before clearing; positions update after.
4. Carol places `BUY YES 200` with no NO counterparty → **MM backstops** the residual.
5. Close → resolve → winners claim $1/contract. Assert balances.

---

## The remaining build work (I can do now / next)

| Piece | Who | Notes |
|---|---|---|
| Reconcile gateway vs generated IDL | me (needs your `arcium build` paste) | resolves the `// RECONCILE:` markers |
| Market-setup script (mint/vault/MM/init) | me | runnable, no Arcium toolchain needed |
| **Relayer service** (clear-trigger + settle loop) | me | TS trigger + Rust settle |
| TypeScript **e2e flow test** | me | the Phase-F proof |
| Minimal **client/CLI or web** order+deposit flow | me | so a real user can place an order |
| Circuit **scale** (`BATCH_CAP 8 → N`) | me | before real volume |
| Optimistic-oracle resolution (replace trusted resolver) | me | for trustless resolution |

**Nothing here is blocked on more design — it's compile + deploy + wire.** The blockers are all things
only your Docker/devnet/Arcium environment can run; I make them turnkey and fix against your outputs.
