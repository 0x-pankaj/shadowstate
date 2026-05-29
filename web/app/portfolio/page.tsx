"use client";

import { useWallet } from "@solana/wallet-adapter-react";
import { usePortfolio } from "@/lib/usePortfolio";
import { usd } from "@/lib/format";
import { Header } from "@/components/Header";
import { PortfolioRow } from "@/components/PortfolioRow";
import { Panel, Spinner, Badge } from "@/components/ui";

export default function PortfolioPage() {
  const { connected } = useWallet();
  const p = usePortfolio();

  const totalClaimable = p.claimable.reduce((a, h) => a + h.claimable, 0n);

  return (
    <main className="min-h-screen">
      <Header />

      <div className="mx-auto max-w-4xl px-5 py-7">
        <div className="mb-6 flex flex-wrap items-end justify-between gap-3">
          <div>
            <h1 className="text-2xl font-black tracking-tight text-ink">Your portfolio</h1>
            <p className="mt-1 text-sm text-muted">
              Positions across every market. Resolved winners claim here — losses settle automatically, nothing to do.
            </p>
          </div>
          <div className="flex items-center gap-2 text-xs text-muted">
            {p.loading && <Spinner />}
            <span>{p.loading ? "loading…" : "auto-refresh 15s"}</span>
          </div>
        </div>

        {!connected ? (
          <Panel>
            <p className="text-sm text-muted">Connect your wallet to see your positions and claims.</p>
          </Panel>
        ) : (
          <div className="flex flex-col gap-5">
            {/* Claimable */}
            <Panel
              title="Ready to claim"
              hint="Resolved markets where you hold winning contracts (or an invalid-market refund)."
              right={
                totalClaimable > 0n ? <Badge tone="brand">{usd(totalClaimable)} total</Badge> : <Badge tone="neutral">0</Badge>
              }
            >
              {p.claimable.length === 0 ? (
                <p className="text-sm text-muted">Nothing to claim right now.</p>
              ) : (
                <div className="flex flex-col gap-2">
                  {p.claimable.map((h) => (
                    <PortfolioRow key={h.address} holding={h} onChange={p.refresh} />
                  ))}
                </div>
              )}
            </Panel>

            {/* Open positions */}
            <Panel title="Open positions" hint="Markets still trading.">
              {p.open.length === 0 ? (
                <p className="text-sm text-muted">No open positions.</p>
              ) : (
                <div className="flex flex-col gap-2">
                  {p.open.map((h) => (
                    <PortfolioRow key={h.address} holding={h} onChange={p.refresh} />
                  ))}
                </div>
              )}
            </Panel>

            {/* Settled losses — informational */}
            {p.settled.length > 0 && (
              <Panel title="Settled" hint="Resolved markets with nothing to claim (already deducted at trade time).">
                <div className="flex flex-col gap-2 opacity-70">
                  {p.settled.map((h) => (
                    <PortfolioRow key={h.address} holding={h} onChange={p.refresh} />
                  ))}
                </div>
              </Panel>
            )}

            {p.error && <div className="rounded-xl border border-no/40 bg-no/10 px-4 py-3 text-sm text-no">{p.error}</div>}
          </div>
        )}
      </div>
    </main>
  );
}
