"use client";

import Link from "next/link";
import { useMarket } from "@/lib/useMarket";
import { marketLabel } from "@/lib/markets";
import { Header } from "@/components/Header";
import { MarketCard } from "@/components/MarketCard";
import { Portfolio } from "@/components/Portfolio";
import { OrderTicket } from "@/components/OrderTicket";
import { Collateral } from "@/components/Collateral";
import { Claim } from "@/components/Claim";
import { Faucet } from "@/components/Faucet";
import { Spinner } from "@/components/ui";

export default function MarketDetail({ params }: { params: { address: string } }) {
  const address = decodeURIComponent(params.address);
  const s = useMarket(address);
  const label = marketLabel(address);

  return (
    <main className="min-h-screen">
      <Header sol={s.sol} />

      <div className="mx-auto max-w-6xl px-5 py-7">
        <div className="mb-5 flex flex-wrap items-end justify-between gap-3">
          <div>
            <Link href="/" className="text-xs text-muted hover:text-ink">
              ← All markets
            </Link>
            <h1 className="mt-1 text-xl font-bold tracking-tight text-ink">
              {label?.title ?? "Market detail"}
            </h1>
          </div>
          <div className="flex items-center gap-2 text-xs text-muted">
            {s.loading && <Spinner />}
            <span>{s.loading ? "syncing…" : "live · auto-refresh 12s"}</span>
          </div>
        </div>

        {s.error && (
          <div className="mb-6 rounded-2xl border border-no/40 bg-no/10 px-5 py-4 text-sm text-no">{s.error}</div>
        )}

        {/* Funds flow: how money moves through the app. */}
        <div className="mb-5 flex flex-wrap items-center gap-2 rounded-xl border border-line bg-panel/60 px-4 py-2.5 text-[11px] text-muted">
          <span className="font-semibold text-brand2">Flow</span>
          <span>🚰 Mint test USDC</span>
          <span>→</span>
          <span>💼 Wallet</span>
          <span>→</span>
          <span>🏦 Deposit to vault (collateral)</span>
          <span>→</span>
          <span>🔒 Seal order</span>
          <span>→</span>
          <span>💰 Claim winnings</span>
        </div>

        <div className="grid grid-cols-1 gap-5 lg:grid-cols-3">
          <div className="flex flex-col gap-5 lg:col-span-2">
            <MarketCard market={s.market} marketAddr={address} />
            <Portfolio
              market={s.market}
              marketKey={s.marketKey}
              position={s.position}
              positionExists={s.positionExists}
              onChange={s.refresh}
            />
            <Claim market={s.market} marketKey={s.marketKey} position={s.position} onChange={s.refresh} />
          </div>

          <div className="flex flex-col gap-5">
            <OrderTicket marketKey={s.marketKey} />
            <Faucet mint={s.mintKey} onChange={s.refresh} />
            <Collateral
              market={s.market}
              marketKey={s.marketKey}
              walletBalance={s.collateralBalance}
              positionExists={s.positionExists}
              onChange={s.refresh}
            />
          </div>
        </div>
      </div>
    </main>
  );
}
