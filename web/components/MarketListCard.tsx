"use client";

import Link from "next/link";
import { MarketEntry } from "@/lib/discover";
import { OUTCOME, STATUS, SCALE } from "@/lib/constants";
import { qty, short } from "@/lib/format";
import { useMarketMeta } from "@/lib/meta";
import { Badge } from "./ui";

function statusBadge(outcome: number, status: number) {
  if (status === STATUS.CLOSED) return <Badge tone="neutral">Closed</Badge>;
  switch (outcome) {
    case OUTCOME.YES_WON:
      return <Badge tone="yes">YES won</Badge>;
    case OUTCOME.NO_WON:
      return <Badge tone="no">NO won</Badge>;
    case OUTCOME.INVALID:
      return <Badge tone="neutral">Invalid</Badge>;
    default:
      return (
        <Badge tone="brand">
          <span className="live-dot inline-block h-1.5 w-1.5 rounded-full bg-brand2" />
          Trading
        </Badge>
      );
  }
}

export function MarketListCard({ entry }: { entry: MarketEntry }) {
  const { address, market } = entry;
  const label = useMarketMeta().getLabel(address);
  const yesProb = Number(market.baseOraclePrice) / SCALE;
  const title = label?.title ?? `Market ${short(address, 5)}`;

  return (
    <Link
      href={`/market/${address}`}
      className="group flex flex-col rounded-2xl border border-line bg-panel/80 p-5 transition hover:border-brand/50 hover:shadow-glow"
    >
      <div className="mb-3 flex items-start justify-between gap-3">
        <div>
          {label?.category && <div className="mb-1 text-[11px] font-medium uppercase tracking-wider text-brand2">{label.category}</div>}
          <h3 className="text-[15px] font-semibold leading-snug text-ink group-hover:text-white">{title}</h3>
          <div className="mt-0.5 font-mono text-[11px] text-muted">{short(address, 6)}</div>
        </div>
        {statusBadge(market.outcome, market.status)}
      </div>

      <div className="mb-2 flex items-center justify-between text-xs">
        <span className="font-semibold text-yes">YES {(yesProb * 100).toFixed(0)}¢</span>
        <span className="font-semibold text-no">NO {((1 - yesProb) * 100).toFixed(0)}¢</span>
      </div>
      <div className="flex h-2.5 overflow-hidden rounded-full border border-line bg-panel2">
        <div className="bg-yes/70" style={{ width: `${yesProb * 100}%` }} />
        <div className="bg-no/70" style={{ width: `${(1 - yesProb) * 100}%` }} />
      </div>

      <div className="mt-4 flex items-center justify-between text-[11px] text-muted">
        <span>YES vol {qty(market.totalYesSupply)}</span>
        <span>NO vol {qty(market.totalNoSupply)}</span>
      </div>
    </Link>
  );
}
