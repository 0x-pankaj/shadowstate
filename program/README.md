# shadowstate-program

The **on-chain settlement engine** — pure [Pinocchio](https://github.com/anza-xyz/pinocchio) `#![no_std]`,
zero heap, zero-copy. It verifies a committee-signed batch from the off-chain MPC layer, runs
deterministic two-tier clearing, mutates a `bytemuck::Pod` position ledger, and settles collateral in
Token-2022. **No Anchor, no `solana-program`.**

Status: ✅ **Real & deployable.** Compiles to an SBF artifact; 20 unit + LiteSVM integration tests
run against the real bundled Token-2022 program and the Ed25519 precompile.

## Instruction set (1-byte discriminators)

| Disc | Instruction | Auth | Effect |
|---|---|---|---|
| 0 | `InitializeMarket` | authority | Create `MarketState` + immutable `Committee` + vault; set params, oracle, committee, threshold. |
| 1 | `InitUserPosition` | user | Create the per-user position PDA. |
| 2 | `DepositCollateral` | user | `TransferChecked` user → vault; credit collateral. |
| 3 | `SubmitBatch` | relayer + committee sig | **The FBA settlement** (below). |
| 4 | `UpdateRiskParams` | authority | Retune `base_oracle_price`, `max_skew_premium`, `imbalance_threshold` (the `mm-gateway` target). |
| 5 | `WithdrawCollateral` | user | `TransferChecked` vault → user (`invoke_signed`); debit collateral. |
| 6 | `DepositMmCollateral` | any | Fund the MM backstop pool that fully collateralizes Tier-2 residuals. |
| 7 | `ResolveMarket` | authority | Declare the final outcome (YES or NO won); final, one-time. |
| 8 | `ClaimWinnings` | user | Redeem winning contracts for `$1` each (`TransferChecked` vault → user). |
| 9 | `ClaimMmWinnings` | authority | Redeem the MM's winning backstop side for `$1`/contract. |
| 10 | `SubmitBatchTrusted` | settlement authority | Settle a batch via the registered gateway authority — **no committee signatures** (the Arcium path). |
| 11 | `CloseMarket` | authority | End the trading window (one-way): no further settlement, resolution becomes possible. |
| 12 | `WithdrawMmCollateral` | authority | MM reclaims its unreserved backstop float from the vault. |

### Market lifecycle

`TRADING → CLOSED → resolved`. Settlement (`SubmitBatch`/`SubmitBatchTrusted`) requires `STATUS_TRADING`;
`ResolveMarket` requires `STATUS_CLOSED`. `CloseMarket` flips the one-way `status`. Outcomes:
`YES_WON`/`NO_WON` pay `$1`/contract on the winning side; **`INVALID`** voids the market and settles
*every* contract at the `$0.50` midpoint (solvent because `yes_total == no_total`). A full lifecycle
conserves value exactly — the `conservation_invariant_full_lifecycle` test drains the vault to zero.

### Two settlement trust paths

`SubmitBatch` (committee) and `SubmitBatchTrusted` (gateway authority) share the **identical**
deterministic settlement core (`settle::apply_settlement`) — same two-tier math, same full
collateralization, both re-derive economics from the fills and never trust the header. Only
*authentication* differs:
- **Committee** — ≥ threshold Ed25519 committee signatures over the frame (self-operated nodes).
- **Trusted** — a transaction signed by `market.settlement_authority` (set at init; all-zero ⇒
  disabled). Used with real Arcium, where the trust anchor is the MXE attestation verified off-chain
  in the `shadowstate_mxe` gateway. `mpc-core::relay::build_trusted_transaction` builds this transaction.

## `SubmitBatch` — the core

1. Parse + length-validate the frame (`protocol::validate_frame_len`).
2. Bind `header.market` to the `MarketState` PDA; validate vault / MM / mint / PDAs.
3. **Replay guard:** `header.epoch > market.last_epoch`.
4. **Native committee verification** (`sig.rs`): introspect the Instructions sysvar, find the Ed25519
   precompile instructions, confirm ≥ `threshold` *distinct committee members* signed the **exact**
   frame bytes. The runtime already cryptographically verified the signatures via the precompile — the
   program only authorizes *who signed what*.
5. **Re-derive economics** from the fills (one-sided residual, `Σ residual == net_imbalance`); the
   header is never trusted in isolation.
6. **Tier-2 PropAMM price** (`math.rs`), all `checked_*`:
   ```text
   skew_ratio  = min(net * SCALE / imbalance_threshold, SCALE)
   premium     = max_skew_premium * skew_ratio / SCALE
   clear_price = clamp(base ± premium, MIN_PRICE, MAX_PRICE)   // + if YES-heavy
   ```
7. **Apply per fill:** Tier-1 P2P at `$0.50`, Tier-2 residual at the clearing price; MM takes the
   opposite side. Mutate the position ledger (checked).
8. Update market aggregates + MM backstop + `last_epoch`.
9. **Reserve the MM backstop** so the vault fully collateralizes the residual to `$1`/contract:
   `mm_obligation = (SCALE − heavy_price)·net` is debited from `market.mm_collateral` (tokens the MM
   pre-deposited). No token moves in settlement — it is a pure ledger update. (There is *no* spread-fee
   transfer: paying the MM out of the vault would double-count the premium and under-collateralize the
   residual. The MM's edge is the pricing premium — it posts *less* backstop as the premium widens.)

## Full collateralization & resolution

Every contract is backed by `$1` in the vault: a Tier-1 YES/NO pair (each pays `$0.50`) and a Tier-2
residual (buyer pays `heavy_price`, the MM posts the `(1 − heavy_price)` complement). So
`yes_total == no_total` and the vault is always solvent for exactly one outcome's payout.
`ResolveMarket` sets the outcome; `ClaimWinnings` / `ClaimMmWinnings` redeem winning contracts for
`$1` each from the vault (signed by the vault PDA) and zero the settled legs. Losing legs pay `$0`.
A trusted resolver (`market.authority`) sets the outcome today — the hook for an oracle / optimistic
resolution module.

## State accounts (zero-copy `#[repr(C)]` Pod)

- `MarketState` (208 B) — config + running aggregates + MM backstop + replay epoch.
- `Committee` (264 B) — immutable set of trusted MPC node Ed25519 addresses + threshold.
- `UserPosition` (104 B) — per-user YES/NO + collateral ledger.

Account layout convention: `disc:u8 | version:u8 | …`, cast from the 8-aligned buffer at offset 0;
explicit `_pad`/`_reserved` fields forbid implicit padding (`assert_no_padding!` enforces sizes).

## Build & test

```bash
# SBF artifact — edition 2024 needs the v1.52 platform-tools (default v1.51 ships cargo 1.84)
cargo build-sbf --manifest-path program/Cargo.toml --tools-version v1.52

# Unit (math) + LiteSVM integration tests (loads the .so, real Token-2022 + Ed25519 precompile)
cargo test -p shadowstate-program
```

The LiteSVM suite covers init/deposit, full two-tier settlement (YES-heavy and NO-heavy, asserting
positions/MM/totals/the vault→MM fee transfer), and security negatives: replay, sub-threshold
signatures, non-committee signer, and a tampered frame.

## Not yet implemented (for a production prediction market)

- **On-chain sealed-order ingestion** instruction (clients writing into per-user ingestion PDAs;
  with real Arcium this is the `shadowstate_mxe` gateway's `ingest_order`).
- `INVALID`/cancelled-market refunds (today only `YES_WON` / `NO_WON` payout is implemented).
- Order cancellation / cross-epoch resting orders; an optimistic-oracle resolution module.

## License

MIT.
