// Fund the faucet authority with devnet SOL so it can pay fees + ATA rent when self-minting.
//   node scripts/fund-faucet.mjs            (run from web/)
// Transfers from ~/.config/solana/id.json; falls back to a devnet airdrop if that's empty.
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { Connection, Keypair, PublicKey, SystemProgram, Transaction, sendAndConfirmTransaction, LAMPORTS_PER_SOL } from "@solana/web3.js";

const env = {};
for (const l of readFileSync(".env.local", "utf8").split("\n")) { const m = l.match(/^([A-Z0-9_]+)=(.*)$/); if (m) env[m[1]] = m[2]; }
const conn = new Connection(env.NEXT_PUBLIC_RPC_URL, "confirmed");
const auth = Keypair.fromSecretKey(Uint8Array.from(JSON.parse(env.NEXT_PUBLIC_FAUCET_AUTHORITY_SECRET)));
const TARGET = 0.2 * LAMPORTS_PER_SOL;

const before = await conn.getBalance(auth.publicKey);
console.log("Faucet authority:", auth.publicKey.toBase58(), "balance:", before / LAMPORTS_PER_SOL, "SOL");
if (before >= TARGET) { console.log("Already funded. Done."); process.exit(0); }

try {
  const payer = Keypair.fromSecretKey(Uint8Array.from(JSON.parse(readFileSync(`${homedir()}/.config/solana/id.json`, "utf8"))));
  const payerBal = await conn.getBalance(payer.publicKey);
  console.log("Payer (id.json):", payer.publicKey.toBase58(), "balance:", payerBal / LAMPORTS_PER_SOL, "SOL");
  if (payerBal > TARGET + 0.01 * LAMPORTS_PER_SOL) {
    const tx = new Transaction().add(SystemProgram.transfer({ fromPubkey: payer.publicKey, toPubkey: auth.publicKey, lamports: TARGET - before }));
    const sig = await sendAndConfirmTransaction(conn, tx, [payer]);
    console.log("✅ Transferred. sig:", sig);
  } else {
    console.log("Payer too low; trying airdrop…");
    const sig = await conn.requestAirdrop(auth.publicKey, TARGET);
    await conn.confirmTransaction(sig, "confirmed");
    console.log("✅ Airdropped. sig:", sig);
  }
} catch (e) {
  console.log("Transfer failed, trying airdrop:", e.message);
  const sig = await conn.requestAirdrop(auth.publicKey, TARGET);
  await conn.confirmTransaction(sig, "confirmed");
  console.log("✅ Airdropped. sig:", sig);
}
console.log("Final balance:", (await conn.getBalance(auth.publicKey)) / LAMPORTS_PER_SOL, "SOL");
