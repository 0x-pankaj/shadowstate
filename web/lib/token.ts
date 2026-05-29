import { Connection, PublicKey, TransactionInstruction } from "@solana/web3.js";
import {
  TOKEN_2022_PROGRAM_ID,
  ASSOCIATED_TOKEN_PROGRAM_ID,
  getAssociatedTokenAddressSync,
  createAssociatedTokenAccountIdempotentInstruction,
} from "@solana/spl-token";

/** The user's Token-2022 ATA for `mint`. */
export function userAta(mint: PublicKey, owner: PublicKey): PublicKey {
  return getAssociatedTokenAddressSync(mint, owner, true, TOKEN_2022_PROGRAM_ID, ASSOCIATED_TOKEN_PROGRAM_ID);
}

/** An idempotent create-ATA instruction (no-op if it already exists), or null if it exists. */
export async function maybeCreateAta(
  conn: Connection,
  payer: PublicKey,
  mint: PublicKey,
  owner: PublicKey
): Promise<{ ata: PublicKey; ix: TransactionInstruction | null }> {
  const ata = userAta(mint, owner);
  const info = await conn.getAccountInfo(ata);
  const ix = info
    ? null
    : createAssociatedTokenAccountIdempotentInstruction(
        payer,
        ata,
        owner,
        mint,
        TOKEN_2022_PROGRAM_ID,
        ASSOCIATED_TOKEN_PROGRAM_ID
      );
  return { ata, ix };
}

/** Read a Token-2022 account's raw `amount` (base units) — bytes 64..72. */
export async function tokenBalance(conn: Connection, account: PublicKey): Promise<bigint> {
  const info = await conn.getAccountInfo(account);
  if (!info || info.data.length < 72) return 0n;
  return Buffer.from(info.data).readBigUInt64LE(64);
}
