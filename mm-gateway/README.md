# shadowstate-mm-gateway

The **institutional liquidity hub + cross-venue hedging relayer** — the market-maker's control plane
around its on-chain Tier-2 backstop role.

> Status: ⚠️ **Real logic, fully tested; network adapters not yet run against live endpoints.** The
> hedging / risk / portal math is unit- and integration-tested offline with mocks. The WebSocket,
> HTTP, and JSON-RPC adapters are real code but have not been exercised against live Arcium /
> Polymarket / a Solana RPC node.

## What it does

Every batch with a one-sided residual forces the MM to take the **opposite** side of the heavy flow
on-chain. This crate:

1. **Ingests settlement events** — a `tokio-tungstenite` worker hooks an Arcium log stream and decodes
   each settlement line (or raw frame) into a `SettlementEvent`.
2. **Delta-hedges** — reads the direction + size the MM just absorbed and computes the exact
   offsetting order: buy the *heavy side* the MM is short, sized to the residual, limit-priced at the
   on-chain Tier-2 clearing price (re-derived with the program's exact fixed-point math) so filling at
   or below it locks the captured spread.
3. **Clears cross-venue** — dispatches that order to an external lit venue (Polymarket / Kalshi /
   sportsbook) via the `VenueClient` trait.
4. **Retunes risk on-chain** — the parameter-modification portal builds + signs the `UpdateRiskParams`
   transaction so the authorized MM wallet can widen `max_skew_premium` / tighten `imbalance_threshold`
   as implied volatility shifts.

## Modules

| Module | Role |
|---|---|
| `event` | `SettlementEvent` from an on-chain frame or an Arcium WS JSON log (`{"frame":"<base64>"}`). |
| `risk` | `RiskParams` with the exact on-chain validation bounds + `LinearVolPolicy` (implied-vol → params, clamped). |
| `hedge` | The delta-hedging engine; mirrors the program's Tier-2 pricing to size the offset. |
| `portal` | Builds + signs the on-chain `UpdateRiskParams` ix/tx (byte-exact 24-byte payload). |
| `venue` | `VenueClient` trait + real `reqwest` `HttpVenueClient` + `MockVenue`. |
| `rpc` | `ChainSubmitter` trait + minimal Solana JSON-RPC `RpcSubmitter` (no `solana-client`) + `MockSubmitter`. |
| `stream` | `EventStream` trait + `tokio-tungstenite` `WsEventStream` + `MemoryStream`. |
| `gateway` | The `Gateway` orchestrator: hedge loop + retune portal, generic over the three network traits. |

## Design note: no `solana-client`

The on-chain `UpdateRiskParams` transaction is built from the granular Solana crates and broadcast
through a small JSON-RPC submitter over the `reqwest` client already needed for venue orders — keeping
the dependency tree lean and the crate offline-testable. Network boundaries (`EventStream`,
`VenueClient`, `ChainSubmitter`) are traits, so the hedging and risk math are fully tested with mocks.

## Build & test

```bash
cargo test --manifest-path mm-gateway/Cargo.toml
```

31 tests. `tests/end_to_end.rs` proves a real YES-heavy frame drives a buy-YES-200-@-$0.52 hedge whose
captured premium equals the on-chain MM fee, and that `retune` broadcasts a real `UpdateRiskParams`
transaction with the correct param bytes.

## Going live (next phase)

- Point `WsEventStream` at the real Arcium log endpoint and confirm the settlement log shape.
- Implement `VenueClient` for each target venue's real API (auth, symbol mapping, order types) — the
  `HttpVenueClient` is a generic `POST /orders` placeholder.
- Point `RpcSubmitter` at a funded RPC endpoint and confirm fee-payer funding / retries / confirmation.

## License

MIT.
