# shadowstate-mpc

The **off-chain confidential engine**. It privately aggregates orders, matches overlapping YES/NO
demand peer-to-peer, computes the residual the market-maker backstops, and emits a
committee-signed batch frame the on-chain program verifies natively and settles.

> Status: ⚠️ **Faithful local model of an Arcium MXE — not a live network connection.** It implements
> the *cryptographic primitive Arcium is built on* (additive secret sharing) in one process, and
> produces frames + Ed25519 signatures the real on-chain program accepts byte-for-byte. Going live on
> Arcium's decentralized network requires their toolchain — see [below](#running-on-real-arcium).

## Why not `arcium-client`?

The official SDK is **rejected** here for three concrete reasons — the same supply-chain diligence
applied across this project:

1. Its default feature (`transactions`) pulls in `anchor-client` + `anchor-spl` — a direct violation
   of the project's absolute **No-Anchor** law.
2. It is **GPL-3.0-or-later**, incompatible with this crate's MIT license (linking would virally
   relicense the binary).
3. It needs a **live MXE cluster + a compiled Arcis circuit** to do anything, so it can't be part of
   a self-contained, offline-testable crate.

Instead the MXE's computation is implemented natively and correctly.

## Modules

| Module | Role |
|---|---|
| `seal` | Client order sealing: real **x25519 ECDH + ChaCha20Poly1305** (how an Arcium client encrypts inputs to the cluster). |
| `secret_share` | **Additive secret sharing** over `Z/2^64` + a SplitMix64 blind generator — the "blinded accumulator, no plaintext" primitive. Accumulation is share-local; only final aggregates are revealed. |
| `order` | Plaintext `Order` + its fixed 81-byte sealed/ingestion byte layout. |
| `mxe` | The matching matrix: blinded accumulate → reveal aggregates → private P2P match → exact integer residual allocation (`Σ residual == net`, no rounding drift). |
| `committee` | Threshold **Ed25519** frame signing; off-chain quorum pre-flight mirror. |
| `frame` | Byte-identical `BatchHeader ++ UserFill` assembly, verified through the real `protocol` readers. |
| `relay` | Ed25519-precompile instruction (byte-mirror of the on-chain parser) + `SubmitBatch` instruction + signed transaction. |
| `relayer` | **Production relayer**: turns the Arcium gateway's revealed `BatchCleared` (`ClearedBatch` + `SlotRegistry`) into a committee-signed settlement — reconstruct orders → deterministic re-derive → cross-check vs gateway aggregates → frame → sign. |
| `engine` | One-call epoch pipeline (self-operated-committee model): unseal → match → frame → sign. |
| `ingestion` | The strict **1200 ms `tokio::time::interval`** FBA loop over a pluggable `IngestionSource`. |

## Two roles: model vs production

This crate plays two roles. `seal`/`secret_share`/`mxe`/`engine`/`ingestion` are the **local MPC
model** (the self-operated-committee path, fully testable offline). With **real Arcium** the matching
moves into the MXE; this crate then becomes the **production relayer** — `relayer` consumes the
gateway's revealed clearing and drives on-chain settlement. The real **client** (sealing orders to the
cluster) is TypeScript in [`../client/`](../client) (RescueCipher lives only in `@arcium-hq/client`).

## Privacy model

Order size and identity are split into additive shares the instant an order is unsealed; every node
holds only shares; accumulation never reconstructs a plaintext order. The single deliberate reveal is
the *aggregate* (total YES, total NO, residual) — which is public on-chain by design anyway. Per-user
fill amounts are public (settlement is plaintext); the *pre-match order book* is what stays private.

## Build & test

```bash
cargo test --manifest-path mpc-core/Cargo.toml
```

39 tests. The `relay`/`frame` tests prove byte-compatibility against re-implementations of the real
on-chain parsers; `tests/end_to_end.rs` proves the produced batch settles to the **exact numbers the
on-chain LiteSVM test asserts** (Alice YES 300 / cost 154, Bob NO 100 / cost 50, MM fee 4).

Built **standalone** (excluded from the workspace) so the on-chain program's pinned lock stays
untouched; it depends on `protocol/` by path so the wire contract cannot drift.

## Running on real Arcium

To replace the local model with the live network:

1. Express the `mxe` matching logic as an **Arcis circuit**; compile + deploy a computation definition
   (MXE) with the `arcium` CLI.
2. Obtain a **cluster** (devnet provides shared clusters) and its x25519 public key; seal orders to it
   in `seal`.
3. Fund a **payer keypair** for metered computations.
4. Populate the on-chain `Committee` with the **cluster's signing keys**, and have the cluster sign
   each computation's output frame — `relay` then submits it unchanged.

No secret credentials are required to build/test this repo; live Arcium is standard onboarding
(CLI + circuit + cluster config + funded payer).

## License

MIT.
