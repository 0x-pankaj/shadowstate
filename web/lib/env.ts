/** Build-time config shared across the app. */

/** Current FBA epoch the gateway opens its batch book for. */
export const EPOCH = BigInt(process.env.NEXT_PUBLIC_EPOCH || "1");

/**
 * Optional seed market (`MarketState` PDA). Used as a fallback when the RPC can't serve
 * `getProgramAccounts` for full discovery. Leave blank once discovery works.
 */
export const DEFAULT_MARKET = process.env.NEXT_PUBLIC_MARKET || "";

// NOTE: Pinata credentials live ONLY in `.env.local` as the server-side `PINATA_JWT` (never here —
// this file is bundled to the browser). Market metadata is pinned/read via `/api/market-meta`.
