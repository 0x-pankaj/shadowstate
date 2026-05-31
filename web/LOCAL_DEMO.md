# ShadowState — running the demo without devnet rate limits

The devnet **program deploy** is what hit the airdrop/RPC rate limit — that's a one-time cost and it's
already done (`FP8ri…` is live). Everyday demo transactions (create market, mint, deposit, withdraw,
claim) are tiny and run fine on devnet **right now**. So the simplest path is:

## Option A — Demo on devnet (recommended; works today)

Everything in the **settlement product** is live on devnet and works with Phantom/Solflare:

1. `cd web && pnpm dev`
2. Connect wallet (set Phantom to **Devnet**).
3. **Create** → fill a question → *Create market* (you become its authority).
4. **Mint test USDC** (home funds panel) → open your market → **Initialize position** → **Deposit**.
5. **Withdraw** to pull collateral back; after you resolve the market, **Claim** pays winners $1/contract.

No rate limits hit here — those only affect the big one-time program deploy, which is finished.

> The **sealed order** button is the only piece not yet live: it calls the Arcium MXE gateway, which
> needs its circuits uploaded + a batch book opened (the confidential-matching path). That part needs
> either Arcium's hosted devnet nodes (paid RPC for the circuit upload) or the Arcium localnet Docker.

## Option B — Fully local validator (zero rate limits, offline)

A local `solana-test-validator` gives unlimited instant airdrops and preloads the program at its exact
ID — no deploy tx at all. Token-2022 + ATA are bundled.

```bash
# Terminal 1 — start the chain with the settlement program preloaded:
cd /home/pankaj/solana-fellowship/shadowstate
solana-test-validator --reset \
  --bpf-program FP8riDGv8jrif5G8QfprfzPeNR8kQ3UUrpTp6EXByDVZ target/deploy/shadowstate_program.so

# Terminal 2 — point the CLI at it and fund yourself:
solana config set --url http://127.0.0.1:8899
solana airdrop 100

# Create the test-USDC mint + a market against localhost:
cd web
NEXT_PUBLIC_RPC_URL=http://127.0.0.1:8899 node scripts/setup-faucet.mjs     # paste output into .env.local
NEXT_PUBLIC_RPC_URL=http://127.0.0.1:8899 NEXT_PUBLIC_FAUCET_MINT=<mint> node scripts/create-market.mjs
```

`.env.local` for local:
```
NEXT_PUBLIC_RPC_URL=http://127.0.0.1:8899
NEXT_PUBLIC_SETTLEMENT_PROGRAM_ID=FP8riDGv8jrif5G8QfprfzPeNR8kQ3UUrpTp6EXByDVZ
NEXT_PUBLIC_FAUCET_MINT=<from setup-faucet>
NEXT_PUBLIC_FAUCET_AUTHORITY_SECRET=<from setup-faucet>
```

**Wallet caveat:** Phantom/Solflare don't connect to `localhost`. For a UI demo on a local validator,
use a wallet that supports a custom RPC (e.g. **Backpack**), or just use **Option A** (devnet) for the
on-camera UI and keep the local validator for CLI/script runs. The dApp broadcasts via its own
`NEXT_PUBLIC_RPC_URL`, so reads/sends hit localhost; only the wallet's own network UI is the friction.

## Why not Docker?

You *can* wrap `solana-test-validator` in Docker exactly like the Arcium localnet, but it adds nothing
over running it natively here (it's a single binary, already installed). Docker only earned its keep
for **Arcium**, whose ARX MPC nodes ship as containers. The settlement chain doesn't need it.
