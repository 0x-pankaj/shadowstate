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

export function marketLabel(address: string): MarketLabel | null {
  return MARKET_LABELS[address] ?? null;
}
