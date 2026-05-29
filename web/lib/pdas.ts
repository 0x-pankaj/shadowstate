import { PublicKey } from "@solana/web3.js";
import { SEED, SETTLEMENT_PROGRAM_ID, GATEWAY_PROGRAM_ID } from "./constants";

const enc = (s: string) => Buffer.from(s, "utf8");

/** MarketState PDA: `[b"market", authority]` (settlement program). */
export function marketPda(authority: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync([enc(SEED.MARKET), authority.toBuffer()], SETTLEMENT_PROGRAM_ID)[0];
}

/** Committee PDA: `[b"committee", market]`. */
export function committeePda(market: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync([enc(SEED.COMMITTEE), market.toBuffer()], SETTLEMENT_PROGRAM_ID)[0];
}

/** Vault authority PDA: `[b"vault", market]` (the vault token account's owner). */
export function vaultAuthorityPda(market: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync([enc(SEED.VAULT), market.toBuffer()], SETTLEMENT_PROGRAM_ID)[0];
}

/** UserPosition PDA: `[b"pos", market, owner]`. */
export function positionPda(market: PublicKey, owner: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync(
    [enc(SEED.POSITION), market.toBuffer(), owner.toBuffer()],
    SETTLEMENT_PROGRAM_ID
  )[0];
}

/** Gateway batch-book PDA: `[b"book", market, epoch_le]`. */
export function bookPda(market: PublicKey, epoch: bigint): PublicKey {
  const e = Buffer.alloc(8);
  e.writeBigUInt64LE(epoch);
  return PublicKey.findProgramAddressSync([enc(SEED.BOOK), market.toBuffer(), e], GATEWAY_PROGRAM_ID)[0];
}
