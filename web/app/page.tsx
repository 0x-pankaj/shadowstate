"use client";

import Link from "next/link";
import { useMarkets } from "@/lib/useMarkets";
import { Header } from "@/components/Header";
import { MarketListCard } from "@/components/MarketListCard";
import { HomeFunds } from "@/components/HomeFunds";
import { Spinner } from "@/components/ui";

export default function Page() {
  const { markets, loading, error } = useMarkets();

  return (
    <main className="min-h-screen">
      <Header />

      <div className="mx-auto max-w-6xl px-5 py-7">
        <div className="mb-6 flex flex-wrap items-end justify-between gap-3">
          <div>
            <h1 className="bg-brand-grad bg-clip-text text-2xl font-black tracking-tight text-transparent">
              Trade the unseen.
            </h1>
            <p className="mt-1 max-w-xl text-sm text-muted">
              Confidential prediction markets. Pick a market, place a sealed YES/NO order matched privately in an
              Arcium MPC, settled trustlessly on Solana.
            </p>
          </div>
          <div className="flex items-center gap-2 text-xs text-muted">
            {loading && <Spinner />}
            <span>{loading ? "scanning markets…" : `${markets.length} market${markets.length === 1 ? "" : "s"}`}</span>
          </div>
        </div>

        {/* Global funds + faucet — usable before any market exists. */}
        <div className="mb-6">
          <HomeFunds />
        </div>

        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-sm font-semibold tracking-wide text-ink">Markets</h2>
          <Link
            href="/create"
            className="rounded-lg bg-brand-grad px-3 py-1.5 text-xs font-semibold text-bg hover:brightness-110"
          >
            + Create market
          </Link>
        </div>

        {error && !loading && markets.length === 0 && (
          <div className="rounded-2xl border border-no/40 bg-no/10 px-5 py-4 text-sm text-no">{error}</div>
        )}

        {!loading && markets.length === 0 && !error && (
          <div className="rounded-2xl border border-line bg-panel px-5 py-8 text-sm text-muted">
            <p className="font-medium text-ink">No markets yet.</p>
            <p className="mt-1">
              Deposit, withdraw, ordering and claims all live <em>inside</em> a market. Create one on-chain to begin:
            </p>
            <pre className="mt-3 overflow-x-auto rounded-lg border border-line bg-bg px-3 py-2 font-mono text-xs text-brand2">
{`# after deploying the settlement program:
cd web && pnpm market:create`}
            </pre>
            <p className="mt-2 text-xs">It’ll print the market address and auto-appear here.</p>
          </div>
        )}

        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {markets.map((m) => (
            <MarketListCard key={m.address} entry={m} />
          ))}
        </div>
      </div>
    </main>
  );
}
