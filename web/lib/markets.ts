/**
 * Optional human labels for markets. On-chain `MarketState` stores only numbers + keys (no
 * question text), so the operator can map a market PDA → a readable question here. Markets not
 * listed still render, keyed by their address.
 *
 * Fill in after creating a market on devnet, e.g.:
 *   "9xQ…abc": { title: "Will ETH close above $4k on Jun 30?", category: "Crypto" },
 */
export interface MarketLabel {
  title: string;
  category?: string;
  description?: string;
}

export const MARKET_LABELS: Record<string, MarketLabel> = {
  // "<MarketState PDA base58>": { title: "…", category: "…" },
};

const LS_KEY = "ss:market-labels";

/** Labels for markets created from the browser are persisted client-side in localStorage. */
function localLabels(): Record<string, MarketLabel> {
  if (typeof window === "undefined") return {};
  try {
    return JSON.parse(window.localStorage.getItem(LS_KEY) || "{}");
  } catch {
    return {};
  }
}

export function marketLabel(address: string): MarketLabel | null {
  return MARKET_LABELS[address] ?? localLabels()[address] ?? null;
}

/** Persist a question/title for a market the user just created (client-side only). */
export function saveMarketLabel(address: string, label: MarketLabel): void {
  if (typeof window === "undefined") return;
  try {
    const all = localLabels();
    all[address] = label;
    window.localStorage.setItem(LS_KEY, JSON.stringify(all));
  } catch {
    /* ignore */
  }
}
