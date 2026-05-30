// Print the faucet authority's devnet SOL balance (run from web/: node scripts/check-faucet-balance.mjs)
import { readFileSync } from "node:fs";
import { Connection, Keypair } from "@solana/web3.js";

const env = {};
for (const l of readFileSync(".env.local", "utf8").split("\n")) {
  const m = l.match(/^([A-Z0-9_]+)=(.*)$/);
  if (m) env[m[1]] = m[2];
}
const conn = new Connection(env.NEXT_PUBLIC_RPC_URL, "confirmed");
const auth = Keypair.fromSecretKey(Uint8Array.from(JSON.parse(env.NEXT_PUBLIC_FAUCET_AUTHORITY_SECRET)));
const bal = await conn.getBalance(auth.publicKey);
console.log("Faucet authority:    ", auth.publicKey.toBase58());
console.log("Faucet authority SOL:", bal / 1e9);
