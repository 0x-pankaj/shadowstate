// Prove the self-signing faucet mint works: build the exact tx the app builds and SIMULATE it
// against the configured RPC (no real send). Run from web/: node scripts/test-mint.mjs
import { readFileSync } from "node:fs";
import { Connection, Keypair, PublicKey, Transaction } from "@solana/web3.js";
import {
  TOKEN_2022_PROGRAM_ID, ASSOCIATED_TOKEN_PROGRAM_ID,
  getAssociatedTokenAddressSync,
  createAssociatedTokenAccountIdempotentInstruction, createMintToInstruction,
} from "@solana/spl-token";

const env = {};
for (const l of readFileSync(".env.local", "utf8").split("\n")) { const m = l.match(/^([A-Z0-9_]+)=(.*)$/); if (m) env[m[1]] = m[2]; }
const conn = new Connection(env.NEXT_PUBLIC_RPC_URL, "confirmed");
const auth = Keypair.fromSecretKey(Uint8Array.from(JSON.parse(env.NEXT_PUBLIC_FAUCET_AUTHORITY_SECRET)));
const mint = new PublicKey(env.NEXT_PUBLIC_FAUCET_MINT);

// Pretend a fresh user wallet is the recipient (worst case: ATA doesn't exist yet → rent paid).
const recipient = Keypair.generate().publicKey;
const ata = getAssociatedTokenAddressSync(mint, recipient, true, TOKEN_2022_PROGRAM_ID, ASSOCIATED_TOKEN_PROGRAM_ID);
const amountBase = 1000n * 1_000_000n;

const tx = new Transaction()
  .add(createAssociatedTokenAccountIdempotentInstruction(auth.publicKey, ata, recipient, mint, TOKEN_2022_PROGRAM_ID, ASSOCIATED_TOKEN_PROGRAM_ID))
  .add(createMintToInstruction(mint, ata, auth.publicKey, amountBase, [], TOKEN_2022_PROGRAM_ID));
tx.feePayer = auth.publicKey;
tx.recentBlockhash = (await conn.getLatestBlockhash("confirmed")).blockhash;
tx.sign(auth);

const sim = await conn.simulateTransaction(tx);
if (sim.value.err) {
  console.log("❌ SIMULATION FAILED:", JSON.stringify(sim.value.err));
  console.log((sim.value.logs ?? []).join("\n"));
  process.exit(1);
}
console.log("✅ SIMULATION OK — self-signing mint of 1000 test USDC to a fresh ATA succeeds.");
console.log("   units consumed:", sim.value.unitsConsumed);
