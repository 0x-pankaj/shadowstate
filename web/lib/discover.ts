import { Connection, PublicKey } from "@solana/web3.js";
import { SETTLEMENT_PROGRAM_ID } from "./constants";
import { decodeMarket, decodePosition, MarketState, UserPosition } from "./state";

export interface MarketEntry {
  pubkey: PublicKey;
  address: string;
  market: MarketState;
}

export interface PositionEntry {
  pubkey: PublicKey;
  position: UserPosition;
}

// base58 of a single discriminator byte: value 1 → "2", value 3 → "4"
// (base58 alphabet index 0 = '1', so byte N encodes to the (N+1)-th symbol).
const DISC_MARKET_B58 = "2"; // MarketState disc == 1
const DISC_POSITION_B58 = "4"; // UserPosition disc == 3

/** Fetch every `MarketState` owned by the settlement program. */
export async function discoverMarkets(conn: Connection): Promise<MarketEntry[]> {
  const accts = await conn.getProgramAccounts(SETTLEMENT_PROGRAM_ID, {
    filters: [{ memcmp: { offset: 0, bytes: DISC_MARKET_B58 } }],
  });
  const out: MarketEntry[] = [];
  for (const { pubkey, account } of accts) {
    const market = decodeMarket(account.data);
    if (market) out.push({ pubkey, address: pubkey.toBase58(), market });
  }
  return out;
}

/** Fetch every `UserPosition` owned by `owner` across all markets. */
export async function discoverUserPositions(conn: Connection, owner: PublicKey): Promise<PositionEntry[]> {
  const accts = await conn.getProgramAccounts(SETTLEMENT_PROGRAM_ID, {
    filters: [
      { memcmp: { offset: 0, bytes: DISC_POSITION_B58 } },
      { memcmp: { offset: 8, bytes: owner.toBase58() } }, // UserPosition.owner @ 8
    ],
  });
  const out: PositionEntry[] = [];
  for (const { pubkey, account } of accts) {
    const position = decodePosition(account.data);
    if (position) out.push({ pubkey, position });
  }
  return out;
}

/** Batch-load + decode a set of market accounts by key. */
export async function loadMarkets(conn: Connection, keys: PublicKey[]): Promise<Map<string, MarketState>> {
  const map = new Map<string, MarketState>();
  if (keys.length === 0) return map;
  // getMultipleAccounts caps at 100 keys per call.
  for (let i = 0; i < keys.length; i += 100) {
    const slice = keys.slice(i, i + 100);
    const infos = await conn.getMultipleAccountsInfo(slice);
    infos.forEach((info, j) => {
      if (info) {
        const m = decodeMarket(info.data);
        if (m) map.set(slice[j].toBase58(), m);
      }
    });
  }
  return map;
}
