import { PublicKey } from "@solana/web3.js";

const pk = (b: Buffer, o: number) => new PublicKey(b.subarray(o, o + 32));
const u64 = (b: Buffer, o: number) => b.readBigUInt64LE(o);

/** Decoded `MarketState` (240-byte zero-copy account). */
export interface MarketState {
  disc: number;
  bump: number;
  vaultBump: number;
  outcome: number;
  status: number;
  authority: PublicKey;
  collateralMint: PublicKey;
  vault: PublicKey;
  mmAccount: PublicKey;
  baseOraclePrice: bigint;
  maxSkewPremium: bigint;
  imbalanceThreshold: bigint;
  totalYesSupply: bigint;
  totalNoSupply: bigint;
  mmYes: bigint;
  mmNo: bigint;
  lastEpoch: bigint;
  mmCollateral: bigint;
  settlementAuthority: PublicKey;
}

export function decodeMarket(data: Buffer | Uint8Array): MarketState | null {
  const b = Buffer.from(data);
  if (b.length < 240 || b[0] !== 1 /* MARKET_STATE disc */) return null;
  return {
    disc: b[0],
    bump: b[2],
    vaultBump: b[3],
    outcome: b[4],
    status: b[5],
    authority: pk(b, 8),
    collateralMint: pk(b, 40),
    vault: pk(b, 72),
    mmAccount: pk(b, 104),
    baseOraclePrice: u64(b, 136),
    maxSkewPremium: u64(b, 144),
    imbalanceThreshold: u64(b, 152),
    totalYesSupply: u64(b, 160),
    totalNoSupply: u64(b, 168),
    mmYes: u64(b, 176),
    mmNo: u64(b, 184),
    lastEpoch: u64(b, 192),
    mmCollateral: u64(b, 200),
    settlementAuthority: pk(b, 208),
  };
}

/** Decoded `UserPosition` (104-byte zero-copy account). */
export interface UserPosition {
  disc: number;
  owner: PublicKey;
  market: PublicKey;
  yesQty: bigint;
  noQty: bigint;
  collateral: bigint;
}

export function decodePosition(data: Buffer | Uint8Array): UserPosition | null {
  const b = Buffer.from(data);
  if (b.length < 104 || b[0] !== 3 /* USER_POSITION disc */) return null;
  return {
    disc: b[0],
    owner: pk(b, 8),
    market: pk(b, 40),
    yesQty: u64(b, 72),
    noQty: u64(b, 80),
    collateral: u64(b, 88),
  };
}
