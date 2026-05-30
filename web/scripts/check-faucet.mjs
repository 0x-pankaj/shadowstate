// Diagnose why the in-app faucet mint fails. Reads web/.env.local and checks, against the
// configured RPC, that the faucet mint exists, is a Token-2022 mint, and that its on-chain
// mint authority matches the authority secret the browser uses to co-sign `mintTo`.
//
//   node scripts/check-faucet.mjs            (run from the web/ directory)

import { readFileSync } from "node:fs";
import { Connection, PublicKey, Keypair } from "@solana/web3.js";
import { getMint, TOKEN_2022_PROGRAM_ID } from "@solana/spl-token";

const env = {};
for (const line of readFileSync(".env.local", "utf8").split("\n")) {
  const m = line.match(/^([A-Z0-9_]+)=(.*)$/);
  if (m) env[m[1]] = m[2];
}

const RPC = env.NEXT_PUBLIC_RPC_URL || "https://api.devnet.solana.com";
const MINT = env.NEXT_PUBLIC_FAUCET_MINT;
const SECRET = env.NEXT_PUBLIC_FAUCET_AUTHORITY_SECRET;

console.log("RPC:                ", RPC);
console.log("FAUCET_MINT:        ", MINT || "(MISSING)");
console.log("AUTHORITY_SECRET:   ", SECRET ? "present" : "(MISSING)");

let authPub = null;
try {
  authPub = Keypair.fromSecretKey(Uint8Array.from(JSON.parse(SECRET))).publicKey;
  console.log("Authority pubkey:   ", authPub.toBase58());
} catch (e) {
  console.log("Authority secret PARSE FAILED:", e.message, "→ faucet is disabled");
}

const conn = new Connection(RPC, "confirmed");
try {
  const mintPk = new PublicKey(MINT);
  const acc = await conn.getAccountInfo(mintPk);
  if (!acc) {
    console.log("RESULT: ❌ mint account does not exist on this RPC/cluster (wrong RPC or re-run setup).");
    process.exit(0);
  }
  console.log(
    "Mint owner program: ",
    acc.owner.toBase58(),
    acc.owner.equals(TOKEN_2022_PROGRAM_ID) ? "(Token-2022 ✓)" : "(NOT Token-2022 ✗)"
  );
  const mint = await getMint(conn, mintPk, "confirmed", TOKEN_2022_PROGRAM_ID);
  console.log("Decimals:           ", mint.decimals);
  console.log("On-chain authority: ", mint.mintAuthority?.toBase58() || "(disabled — cannot mint)");
  if (authPub && mint.mintAuthority) {
    const ok = mint.mintAuthority.equals(authPub);
    console.log("RESULT:", ok ? "✅ authority matches — minting is authorized." : "❌ AUTHORITY MISMATCH — this is why mintTo fails. Re-run setup-faucet and use BOTH new values together.");
  }
} catch (e) {
  console.log("RPC/mint check error:", e.message);
}
