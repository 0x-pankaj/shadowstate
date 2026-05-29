// Create a ShadowState market on devnet (settlement-program `InitializeMarket`, disc 0).
//
//   node scripts/create-market.mjs            # uses env: NEXT_PUBLIC_RPC_URL, _SETTLEMENT_PROGRAM_ID, _FAUCET_MINT
//   node scripts/create-market.mjs <RPC> <SETTLEMENT_PROGRAM_ID> <COLLATERAL_MINT>
//
// Requires the settlement program to be DEPLOYED first (see the deploy checklist). The payer
// (~/.config/solana/id.json) becomes the market authority + the (single) committee member +
// trusted settlement authority. Prints the market PDA → it auto-appears in the UI.

import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import {
  Connection,
  Keypair,
  PublicKey,
  SystemProgram,
  Transaction,
  TransactionInstruction,
  clusterApiUrl,
  sendAndConfirmTransaction,
} from "@solana/web3.js";
import {
  TOKEN_2022_PROGRAM_ID,
  ASSOCIATED_TOKEN_PROGRAM_ID,
  getAssociatedTokenAddressSync,
  createAssociatedTokenAccountIdempotentInstruction,
} from "@solana/spl-token";

const RPC = process.argv[2] || process.env.NEXT_PUBLIC_RPC_URL || clusterApiUrl("devnet");
const PROGRAM_ID = new PublicKey(
  process.argv[3] || process.env.NEXT_PUBLIC_SETTLEMENT_PROGRAM_ID || "FP8riDGv8jrif5G8QfprfzPeNR8kQ3UUrpTp6EXByDVZ"
);
const MINT = new PublicKey(process.argv[4] || process.env.NEXT_PUBLIC_FAUCET_MINT || "");

// Market risk params (6-dec fixed point). Tune as needed.
const BASE_ORACLE_PRICE = 600_000n; // $0.60 Tier-2 anchor
const MAX_SKEW_PREMIUM = 100_000n; // $0.10 max premium at full skew
const IMBALANCE_THRESHOLD = 1_000_000n; // net imbalance (base units) at which skew saturates

const enc = (s) => Buffer.from(s, "utf8");
const u64 = (v) => {
  const b = Buffer.alloc(8);
  b.writeBigUInt64LE(v);
  return b;
};

function loadPayer() {
  const secret = Uint8Array.from(JSON.parse(readFileSync(`${homedir()}/.config/solana/id.json`, "utf8")));
  return Keypair.fromSecretKey(secret);
}

async function main() {
  if (!MINT) throw new Error("Set NEXT_PUBLIC_FAUCET_MINT (or pass the mint as arg 3).");
  const connection = new Connection(RPC, "confirmed");
  const payer = loadPayer();
  const authority = payer.publicKey;

  // PDAs (must mirror the program's seeds + the deployed program id).
  const [market] = PublicKey.findProgramAddressSync([enc("market"), authority.toBuffer()], PROGRAM_ID);
  const [committee] = PublicKey.findProgramAddressSync([enc("committee"), market.toBuffer()], PROGRAM_ID);
  const [vaultPda] = PublicKey.findProgramAddressSync([enc("vault"), market.toBuffer()], PROGRAM_ID);

  // Vault token account = ATA owned by the vault PDA; MM fee account = ATA owned by the operator.
  const vault = getAssociatedTokenAddressSync(MINT, vaultPda, true, TOKEN_2022_PROGRAM_ID, ASSOCIATED_TOKEN_PROGRAM_ID);
  const mmAccount = getAssociatedTokenAddressSync(MINT, authority, true, TOKEN_2022_PROGRAM_ID, ASSOCIATED_TOKEN_PROGRAM_ID);

  console.log("RPC:        ", RPC);
  console.log("Program:    ", PROGRAM_ID.toBase58());
  console.log("Mint:       ", MINT.toBase58());
  console.log("Authority:  ", authority.toBase58());
  console.log("Market PDA: ", market.toBase58());

  const existing = await connection.getAccountInfo(market);
  if (existing) {
    console.log("\n⚠️  Market already exists. Set NEXT_PUBLIC_MARKET=" + market.toBase58());
    return;
  }

  // InitializeMarket data: disc | base_oracle | max_skew | imbalance | count | threshold | members | settlement_auth
  const data = Buffer.concat([
    Buffer.from([0]), // disc INITIALIZE_MARKET
    u64(BASE_ORACLE_PRICE),
    u64(MAX_SKEW_PREMIUM),
    u64(IMBALANCE_THRESHOLD),
    Buffer.from([1]), // committee count = 1
    Buffer.from([1]), // threshold = 1
    authority.toBuffer(), // the single committee member
    authority.toBuffer(), // trusted settlement authority (enables the gateway/relayer path)
  ]);

  const m = (pubkey, isSigner, isWritable) => ({ pubkey, isSigner, isWritable });
  const initIx = new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [
      m(authority, true, true), // payer
      m(authority, true, false), // authority
      m(MINT, false, false),
      m(vault, false, false),
      m(mmAccount, false, false),
      m(market, false, true),
      m(committee, false, true),
      m(SystemProgram.programId, false, false),
    ],
    data,
  });

  const tx = new Transaction()
    .add(
      createAssociatedTokenAccountIdempotentInstruction(
        authority, vault, vaultPda, MINT, TOKEN_2022_PROGRAM_ID, ASSOCIATED_TOKEN_PROGRAM_ID
      )
    )
    .add(
      createAssociatedTokenAccountIdempotentInstruction(
        authority, mmAccount, authority, MINT, TOKEN_2022_PROGRAM_ID, ASSOCIATED_TOKEN_PROGRAM_ID
      )
    )
    .add(initIx);

  const sig = await sendAndConfirmTransaction(connection, tx, [payer], { commitment: "confirmed" });
  console.log("\n✅ Market created. tx:", sig);
  console.log("\n── for web/.env.local (optional fallback) ──");
  console.log("NEXT_PUBLIC_MARKET=" + market.toBase58());
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
