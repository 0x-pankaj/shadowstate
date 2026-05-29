# ShadowState — Web Client

A Next.js 14 dApp for the ShadowState confidential prediction-market dark pool. Connect a
wallet, fund collateral, place **sealed** YES/NO orders (encrypted in-browser, matched
off-chain in the Arcium MPC), track your position, and claim winnings after settlement.

## Stack

- **Next.js 14** (app router) · **React 18** · **TailwindCSS** · **pnpm**
- `@solana/wallet-adapter-*` (Phantom, Solflare) · `@solana/web3.js` · `@solana/spl-token` (Token-2022)
- `@arcium-hq/client` + `@anchor-lang/core` — x25519 + RescueCipher order sealing → gateway `ingest_order`

## Configure

```bash
cp .env.local.example .env.local
```

Fill in:

| Var | What |
|---|---|
| `NEXT_PUBLIC_RPC_URL` | A devnet RPC (use a dedicated provider; the public one drops txs) |
| `NEXT_PUBLIC_GATEWAY_PROGRAM_ID` | Deployed Arcium MXE gateway (default = `E3GF…dNXe`) |
| `NEXT_PUBLIC_SETTLEMENT_PROGRAM_ID` | The Pinocchio settlement engine program ID (used to discover markets) |
| `NEXT_PUBLIC_EPOCH` | The current FBA epoch the gateway book is opened for |
| `NEXT_PUBLIC_MARKET` | *(optional)* a single `MarketState` PDA fallback when the RPC can't serve `getProgramAccounts` |

Markets are **auto-discovered** from the settlement program (`getProgramAccounts`, disc filter), and
each market's collateral mint is read from on-chain state — so no per-market mint config is needed.
Use a provider that supports `getProgramAccounts` (e.g. Helius). Optionally map a market PDA → a
readable question in `lib/markets.ts`.

## Test-USDC faucet (devnet)

Real devnet USDC can't be freely minted, so the app ships its own Token-2022 test mint with a
throwaway faucet authority — any user can self-mint unlimited test collateral from the UI.

```bash
# 1. Create the mint once (uses ~/.config/solana/id.json as payer):
node scripts/setup-faucet.mjs            # or: pnpm faucet:setup
# 2. Paste the printed NEXT_PUBLIC_FAUCET_MINT + NEXT_PUBLIC_FAUCET_AUTHORITY_SECRET into .env.local
# 3. Create your market(s) with collateral_mint == that mint.
```

The faucet authority secret is a **throwaway devnet key** whose only power is minting a worthless
test token — safe to ship in the browser bundle. The **"Test USDC faucet"** panel then appears on
each market page; clicking *Mint* mints to your wallet (you pay the fee, the faucet co-signs).

### Where funds go

```
🚰 Mint test USDC → 💼 your wallet (ATA) → 🏦 Deposit → market vault (collateral, $1 backs each contract)
   → 🔒 Seal & trade → (resolution) → 💰 Claim winnings   ·   ↩ Withdraw frees unused collateral
```

Deposit/withdraw live on each **market detail page** (`/market/[address]`) — collateral is held per
market in that market's Token-2022 vault PDA, credited to your `UserPosition.collateral`.

## Run

```bash
pnpm install
pnpm dev      # http://localhost:3000
pnpm build    # production build
```

## Flow (Polymarket-style)

- **`/` — Markets browser.** Every market as a card with implied YES/NO odds, supplies, and status.
  Click one to open it.
- **`/market/[address]` — Market detail.** The **sealed order ticket is scoped to this market**:
  pick YES/NO + size, encrypted client-side (x25519 + RescueCipher), submitted to the MPC batch via
  the gateway `ingest_order` — side and size never appear on-chain until the epoch clears. Plus
  deposit/withdraw collateral and your position in this market.
- **`/portfolio` — Claims hub.** All your positions across every market in one place:
  - **Ready to claim** — resolved markets where you hold winning contracts (redeem at `$1` each) or
    an `INVALID` refund. One-click claim per row, with a running total.
  - **Open positions** — markets still trading, linking back to trade.
  - **Settled** — resolved losses. **Nothing to claim** — the cost was already deducted at trade
    time, so there's no action (shown only for transparency).

> Order **matching** (`init_book` / `clear_batch`) is performed by the relayer/operator, not the
> client. This UI only seals and submits orders, then reads settled state from chain.
