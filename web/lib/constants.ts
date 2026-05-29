import { PublicKey } from "@solana/web3.js";

/** Token-2022 program. */
export const TOKEN_2022 = new PublicKey("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
/** Associated-token-account program. */
export const ATA_PROGRAM = new PublicKey("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

/** The deployed Arcium MXE gateway program. */
export const GATEWAY_PROGRAM_ID = new PublicKey(
  process.env.NEXT_PUBLIC_GATEWAY_PROGRAM_ID || "E3GFUytcsMFgYgwTrHoob1YvhB4UvqTzj4bFWzE5dNXe"
);

/** The Pinocchio settlement engine. Default = the deploy keypair address (devnet). */
export const SETTLEMENT_PROGRAM_ID = new PublicKey(
  process.env.NEXT_PUBLIC_SETTLEMENT_PROGRAM_ID || "FP8riDGv8jrif5G8QfprfzPeNR8kQ3UUrpTp6EXByDVZ"
);

export const RPC_URL = process.env.NEXT_PUBLIC_RPC_URL || "https://api.devnet.solana.com";

/** Settlement-program instruction discriminators (1-byte, see `protocol::ids::ix`). */
export const IX = {
  INITIALIZE_MARKET: 0,
  INIT_USER_POSITION: 1,
  DEPOSIT_COLLATERAL: 2,
  SUBMIT_BATCH: 3,
  UPDATE_RISK_PARAMS: 4,
  WITHDRAW_COLLATERAL: 5,
  DEPOSIT_MM_COLLATERAL: 6,
  RESOLVE_MARKET: 7,
  CLAIM_WINNINGS: 8,
  CLAIM_MM_WINNINGS: 9,
  SUBMIT_BATCH_TRUSTED: 10,
  CLOSE_MARKET: 11,
  WITHDRAW_MM_COLLATERAL: 12,
} as const;

/** PDA seed prefixes. */
export const SEED = {
  MARKET: "market",
  COMMITTEE: "committee",
  VAULT: "vault",
  POSITION: "pos",
  BOOK: "book",
} as const;

/** Fixed-point: prices/qty are 6-decimal. $1.00 == 1_000_000. */
export const SCALE = 1_000_000;
export const MIDPOINT = 500_000;

/** Outcomes + lifecycle. */
export const OUTCOME = { UNRESOLVED: 0, YES_WON: 1, NO_WON: 2, INVALID: 3 } as const;
export const STATUS = { TRADING: 0, CLOSED: 1 } as const;

export const SIDE = { YES: 0, NO: 1 } as const;
