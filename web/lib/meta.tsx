"use client";

import { createContext, ReactNode, useCallback, useContext, useEffect, useState } from "react";
import { MarketLabel, MARKET_LABELS } from "./markets";

interface MetaEntry {
  title: string;
  category?: string;
  cid?: string;
}

/** Pin a market's metadata (title/category) to Pinata via the server route. */
export async function pinMarketMeta(market: string, title: string, category?: string): Promise<void> {
  const res = await fetch("/api/market-meta", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ market, title, category }),
  });
  if (!res.ok) {
    const j = await res.json().catch(() => ({}));
    throw new Error(j?.error || `pin failed (${res.status})`);
  }
}

const Ctx = createContext<{ getLabel: (address: string) => MarketLabel | null; refresh: () => void }>({
  getLabel: () => null,
  refresh: () => {},
});

export function useMarketMeta() {
  return useContext(Ctx);
}

const LS_KEY = "ss:market-labels";

function readLocal(): Record<string, MarketLabel> {
  try {
    return JSON.parse(window.localStorage.getItem(LS_KEY) || "{}");
  } catch {
    return {};
  }
}

/** Loads market labels from Pinata (shared) + localStorage (this browser) after mount. Starting
 * empty keeps the first client render === server render (avoids hydration mismatch); labels then
 * stream in on the next tick. */
export function MarketMetaProvider({ children }: { children: ReactNode }) {
  const [remote, setRemote] = useState<Record<string, MetaEntry>>({});
  const [local, setLocal] = useState<Record<string, MarketLabel>>({});

  const refresh = useCallback(() => {
    setLocal(readLocal());
    fetch("/api/market-meta", { cache: "no-store" })
      .then((r) => r.json())
      .then((d) => setRemote(d.markets ?? {}))
      .catch(() => {});
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const getLabel = useCallback(
    (address: string): MarketLabel | null => {
      const r = remote[address];
      if (r?.title) return { title: r.title, category: r.category };
      if (local[address]) return local[address];
      return MARKET_LABELS[address] ?? null;
    },
    [remote, local]
  );

  return <Ctx.Provider value={{ getLabel, refresh }}>{children}</Ctx.Provider>;
}
