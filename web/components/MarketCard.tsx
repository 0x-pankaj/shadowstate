"use client";

import { MarketState } from "@/lib/state";
import { OUTCOME, STATUS, SCALE } from "@/lib/constants";
import { usd, price, qty, short } from "@/lib/format";
import { EPOCH } from "@/lib/env";
import { Panel, Stat, Badge } from "./ui";

function outcomeBadge(outcome: number, status: number) {
  if (status === STATUS.CLOSED) return <Badge tone="neutral">Closed</Badge>;
  switch (outcome) {
    case OUTCOME.YES_WON:
      return <Badge tone="yes">Resolved · YES won</Badge>;
    case OUTCOME.NO_WON:
      return <Badge tone="no">Resolved · NO won</Badge>;
    case OUTCOME.INVALID:
      return <Badge tone="neutral">Resolved · Invalid (refund)</Badge>;
    default:
      return (
        <Badge tone="brand">
          <span className="live-dot inline-block h-1.5 w-1.5 rounded-full bg-brand2" />
          Trading · epoch {EPOCH.toString()}
        </Badge>
      );
  }
}

export function MarketCard({ market, marketAddr }: { market: MarketState | null; marketAddr: string }) {
  if (!market) {
    return (
      <Panel title="Market">
        <p className="text-sm text-muted">No market loaded. Configure NEXT_PUBLIC_MARKET in your environment.</p>
      </Panel>
    );
  }

  const yesProb = Number(market.baseOraclePrice) / SCALE; // Tier-2 fair-value anchor → implied YES
  const noProb = 1 - yesProb;

  return (
    <Panel
      title="Market"
      hint={short(marketAddr, 6)}
      right={outcomeBadge(market.outcome, market.status)}
    >
      {/* Implied YES/NO probability bar (from the on-chain Tier-2 oracle anchor). */}
      <div className="mb-4">
        <div className="mb-1.5 flex items-center justify-between text-xs">
          <span className="font-semibold text-yes">YES {(yesProb * 100).toFixed(0)}¢</span>
          <span className="font-semibold text-no">NO {(noProb * 100).toFixed(0)}¢</span>
        </div>
        <div className="flex h-3 overflow-hidden rounded-full border border-line bg-panel2">
          <div className="bg-yes/70" style={{ width: `${yesProb * 100}%` }} />
          <div className="bg-no/70" style={{ width: `${noProb * 100}%` }} />
        </div>
        <p className="mt-1.5 text-[11px] text-muted">
          Tier-2 oracle anchor {price(market.baseOraclePrice)} · Tier-1 P2P crosses at $0.50
        </p>
      </div>

      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <Stat label="YES supply" value={qty(market.totalYesSupply)} accent="yes" />
        <Stat label="NO supply" value={qty(market.totalNoSupply)} accent="no" />
        <Stat label="MM YES / NO" value={`${qty(market.mmYes)} / ${qty(market.mmNo)}`} accent="brand" />
        <Stat label="MM collateral" value={usd(market.mmCollateral)} />
      </div>
    </Panel>
  );
}
