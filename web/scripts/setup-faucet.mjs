// One-time devnet setup: create ShadowState's own test "USDC" (Token-2022, 6 decimals) with a
// throwaway faucet mint-authority, so any user can self-mint unlimited test collateral in the UI.
//
//   node scripts/setup-faucet.mjs [RPC_URL]
//
// Prints the two env values to paste into web/.env.local. The faucet authority secret is a
// THROWAWAY devnet key whose only power is minting a worthless test token — safe to ship to the
// browser. Never reuse this pattern with a real mint.
//
// After this, create a market on-chain whose collateral_mint == the printed mint, so deposits use it.

import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { Connection, Keypair, clusterApiUrl } from "@solana/web3.js";
import { createMint, TOKEN_2022_PROGRAM_ID } from "@solana/spl-token";

const RPC = process.argv[2] || process.env.NEXT_PUBLIC_RPC_URL || clusterApiUrl("devnet");
const DECIMALS = 6;

function loadPayer() {
  const path = `${homedir()}/.config/solana/id.json`;
  const secret = Uint8Array.from(JSON.parse(readFileSync(path, "utf8")));
  return Keypair.fromSecretKey(secret);
}

async function main() {
  const connection = new Connection(RPC, "confirmed");
  const payer = loadPayer();
  const faucetAuthority = Keypair.generate();

  console.log("RPC:           ", RPC);
  console.log("Payer:         ", payer.publicKey.toBase58());
  console.log("Faucet auth:   ", faucetAuthority.publicKey.toBase58());
  console.log("Creating Token-2022 mint (6 decimals)…");

  const mint = await createMint(
    connection,
    payer,
    faucetAuthority.publicKey, // mint authority = throwaway faucet key
    null, // no freeze authority
    DECIMALS,
    undefined,
    { commitment: "confirmed" },
    TOKEN_2022_PROGRAM_ID
  );

  console.log("\n✅ Test USDC mint created:", mint.toBase58());

  // The faucet is self-signing: it pays fees + ATA rent for every in-app mint, so seed it with
  // a little devnet SOL now. (Top up later any time with `node scripts/fund-faucet.mjs`.)
  const SEED_LAMPORTS = 0.2 * 1e9;
  try {
    const { SystemProgram, Transaction, sendAndConfirmTransaction } = await import("@solana/web3.js");
    const tx = new Transaction().add(
      SystemProgram.transfer({
        fromPubkey: payer.publicKey,
        toPubkey: faucetAuthority.publicKey,
        lamports: SEED_LAMPORTS,
      })
    );
    const sig = await sendAndConfirmTransaction(connection, tx, [payer]);
    console.log(`✅ Funded faucet authority with 0.2 SOL (sig ${sig})`);
  } catch (e) {
    console.log(`⚠️  Could not fund faucet authority (${e.message}). Run scripts/fund-faucet.mjs once you have devnet SOL.`);
  }

  console.log("\n── paste into web/.env.local ──────────────────────────────");
  console.log(`NEXT_PUBLIC_FAUCET_MINT=${mint.toBase58()}`);
  console.log(`NEXT_PUBLIC_FAUCET_AUTHORITY_SECRET=[${faucetAuthority.secretKey.toString()}]`);
  console.log("───────────────────────────────────────────────────────────");
  console.log("\nNext: create a market with collateral_mint =", mint.toBase58());
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
