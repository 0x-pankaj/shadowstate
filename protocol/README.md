# shadowstate-protocol

The **frozen wire contract** shared verbatim by the on-chain settlement engine (`shadowstate-program`)
and the off-chain MPC engine (`shadowstate-mpc`). Changing anything here is a wire-breaking change to
both sides at once — which is exactly why it lives in one tiny, dependency-light (`bytemuck` only),
`#![no_std]` crate with compile-time size assertions.

## Contents

| Module | What it freezes |
|---|---|
| `constants` | Fixed-point scale (`SCALE_FACTOR = 1_000_000`, 6-dec), price guardrails (`MIN/MAX/MIDPOINT_PRICE`), FBA cadence (`EPOCH_SLOTS = 3` ≈ 1.2 s), `MAX_COMMITTEE`, `MAX_FILLS`, direction tags. |
| `frame` | The signed batch payload: `BatchHeader` (72 bytes) `++` `[UserFill; fill_count]` (64 bytes each), little-endian with explicit padding. Plus the readers (`read_header`, `read_fill`, `validate_frame_len`). |
| `ids` | Instruction discriminators (1-byte, Pinocchio convention), account discriminators, PDA seed prefixes, `ACCOUNT_VERSION`. |

## The signed frame

```text
[ BatchHeader : 72 bytes ] [ UserFill : 64 bytes ] × header.fill_count
```

The MPC committee signs **exactly these bytes**. On-chain, the `SubmitBatch` instruction data is
`[disc=3] ++ <frame>`, and the signed message is the frame with the discriminator stripped.

- `BatchHeader.net_imbalance` / `direction` are **advisory** — the on-chain engine re-derives them
  from the fills and rejects the batch on mismatch. Only the committee signature over the raw bytes
  is trusted.
- Instruction data is not guaranteed 8-byte aligned, so the frame is read with *unaligned*
  `bytemuck::pod_read_unaligned` copies (account *state* in `program/` is 8-aligned and cast
  zero-copy instead).

Compile-time guarantees (a mismatch is a build error, catching accidental padding/reordering):

```rust
const _: () = assert!(HEADER_LEN == 72);
const _: () = assert!(FILL_LEN == 64);
```

## Build & test

```bash
cargo test -p shadowstate-protocol     # from the workspace root
```

## License

MIT.
