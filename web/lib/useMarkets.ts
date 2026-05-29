"use client";

import { useCallback, useEffect, useState } from "react";
import { Connection, PublicKey } from "@solana/web3.js";
import { useConnection } from "@solana/wallet-adapter-react";
import { discoverMarkets, MarketEntry } from "./discover";
import { decodeMarket } from "./state";
import { DEFAULT_MARKET } from "./env";

/** List all markets. Falls back to the single seed market if `getProgramAccounts` is unavailable. */
export function useMarkets(): { markets: MarketEntry[]; loading: boolean; error: string | null; refresh: () => void } {
  const { connection } = useConnection();
  const [markets, setMarkets] = useState<MarketEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    setLoading(true);
    load(connection)
      .then(({ markets, error }) => {
        setMarkets(markets);
        setError(error);
      })
      .catch((e) => setError(e?.message ?? String(e)))
      .finally(() => setLoading(false));
  }, [connection]);

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, 20_000);
    return () => clearInterval(id);
  }, [refresh]);

  return { markets, loading, error, refresh };
}

async function load(conn: Connection): Promise<{ markets: MarketEntry[]; error: string | null }> {
  try {
    const markets = await discoverMarkets(conn);
    if (markets.length > 0) return { markets, error: null };
  } catch {
    // RPC may reject getProgramAccounts — fall through to the seed market.
  }
  if (DEFAULT_MARKET) {
    try {
      const key = new PublicKey(DEFAULT_MARKET);
      const info = await conn.getAccountInfo(key);
      const m = info ? decodeMarket(info.data) : null;
      if (m) return { markets: [{ pubkey: key, address: key.toBase58(), market: m }], error: null };
    } catch {
      /* ignore */
    }
  }
  return {
    markets: [],
    error: "No markets found. The RPC may not support getProgramAccounts — set NEXT_PUBLIC_MARKET as a fallback.",
  };
}
