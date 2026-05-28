# ShadowState confidential order client (TypeScript)

The **real** order-submission client. It seals a YES/NO order with the Arcium **RescueCipher** and
calls the `shadowstate_mxe` gateway's `ingest_order` instruction, so the order's side and size stay
hidden inside the MXE until the batch clears.

Why TypeScript: the cluster encryption (RescueCipher over x25519) lives only in `@arcium-hq/client`.
The Rust `mpc-core::seal` (x25519 + ChaCha20Poly1305) is a faithful *model* of this for the
self-operated-committee path; this client is the real Arcium path.

## The two halves of the off-chain pipeline

```
  client (this, TS)                     relayer (mpc-core, Rust)
  seal order  ──ingest_order──▶ MXE ──clear_batch──▶ BatchCleared event
                                                        │
                                          ClearedBatch + SlotRegistry
                                                        │
                                       Relayer::settle → committee-signed frame
                                                        │
                                          program/ SubmitBatch (Pinocchio)
```

This client only does the **ingest** (seal → submit). The **relayer** (revealed clearing →
deterministic pro-rata → on-chain settlement) is the Rust `mpc-core::relayer` module — real and
tested. Together they are the production client + relayer.

## Status & requirements

⚠️ Written to the documented Arcium **0.6.3** API (`@arcium-hq/client`); **not compiled/tested in this
repo** — it needs the Arcium toolchain (Node + `@arcium-hq/client` + `@coral-xyz/anchor`), the deployed
gateway program + its generated IDL/types, and a live MXE cluster (devnet offset 456).

```bash
npm install   # @arcium-hq/client, @coral-xyz/anchor, @solana/web3.js, tweetnacl
```

```ts
import { submitSealedOrder, Side } from "./shadowstate-client";
// program: Anchor Program for the deployed gateway; market: its MarketState PDA
await submitSealedOrder(program, provider, owner, market, 1n /*epoch*/, Side.Yes, 300n /*qty*/);
```

## Reconcile after `arcium build`

The `ingestOrder` argument order and the queue account list mirror `shadowstate_mxe/programs/shadowstate_mxe/src/lib.rs`.
Confirm both against the gateway's generated IDL once it compiles (the gateway's own `// RECONCILE:`
markers). The `book` PDA seeds `[b"book", market, epoch_le]` must match the gateway.
