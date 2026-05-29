import { PublicKey, SystemProgram, TransactionInstruction } from "@solana/web3.js";
import { IX, SETTLEMENT_PROGRAM_ID, TOKEN_2022 } from "./constants";
import { marketPda, positionPda, vaultAuthorityPda } from "./pdas";

const m = (pubkey: PublicKey, isSigner: boolean, isWritable: boolean) => ({ pubkey, isSigner, isWritable });
const u64le = (v: bigint) => {
  const b = Buffer.alloc(8);
  b.writeBigUInt64LE(v);
  return b;
};

/** `InitUserPosition` — create the user's per-market position PDA. */
export function ixInitPosition(payer: PublicKey, owner: PublicKey, market: PublicKey): TransactionInstruction {
  return new TransactionInstruction({
    programId: SETTLEMENT_PROGRAM_ID,
    keys: [
      m(payer, true, true),
      m(owner, true, false),
      m(market, false, false),
      m(positionPda(market, owner), false, true),
      m(SystemProgram.programId, false, false),
    ],
    data: Buffer.from([IX.INIT_USER_POSITION]),
  });
}

/** `DepositCollateral` — `TransferChecked` user→vault, credit position collateral. */
export function ixDeposit(
  owner: PublicKey,
  market: PublicKey,
  userToken: PublicKey,
  vault: PublicKey,
  mint: PublicKey,
  amount: bigint
): TransactionInstruction {
  return new TransactionInstruction({
    programId: SETTLEMENT_PROGRAM_ID,
    keys: [
      m(owner, true, false),
      m(market, false, false),
      m(positionPda(market, owner), false, true),
      m(userToken, false, true),
      m(vault, false, true),
      m(mint, false, false),
      m(TOKEN_2022, false, false),
    ],
    data: Buffer.concat([Buffer.from([IX.DEPOSIT_COLLATERAL]), u64le(amount)]),
  });
}

/** `WithdrawCollateral` — vault→user (signed by vault PDA), debit free collateral. */
export function ixWithdraw(
  owner: PublicKey,
  market: PublicKey,
  userToken: PublicKey,
  vault: PublicKey,
  mint: PublicKey,
  amount: bigint
): TransactionInstruction {
  return new TransactionInstruction({
    programId: SETTLEMENT_PROGRAM_ID,
    keys: [
      m(owner, true, false),
      m(market, false, false),
      m(positionPda(market, owner), false, true),
      m(vault, false, true),
      m(userToken, false, true),
      m(mint, false, false),
      m(vaultAuthorityPda(market), false, false),
      m(TOKEN_2022, false, false),
    ],
    data: Buffer.concat([Buffer.from([IX.WITHDRAW_COLLATERAL]), u64le(amount)]),
  });
}

/** `ClaimWinnings` — redeem winning contracts for $1 each after resolution. */
export function ixClaim(
  owner: PublicKey,
  market: PublicKey,
  userToken: PublicKey,
  vault: PublicKey,
  mint: PublicKey
): TransactionInstruction {
  return new TransactionInstruction({
    programId: SETTLEMENT_PROGRAM_ID,
    keys: [
      m(owner, true, false),
      m(market, false, false),
      m(positionPda(market, owner), false, true),
      m(userToken, false, true),
      m(vault, false, true),
      m(mint, false, false),
      m(vaultAuthorityPda(market), false, false),
      m(TOKEN_2022, false, false),
    ],
    data: Buffer.from([IX.CLAIM_WINNINGS]),
  });
}

export { marketPda, positionPda };
