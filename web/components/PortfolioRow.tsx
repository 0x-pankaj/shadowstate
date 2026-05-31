"use client";

import { useState } from "react";
import Link from "next/link";
import { PublicKey, TransactionInstruction } from "@solana/web3.js";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import { Holding } from "@/lib/usePortfolio";
import { OUTCOME } from "@/lib/constants";
import { ixClaim } from "@/lib/ix";
import { userAta, maybeCreateAta } from "@/lib/token";
import { usd, qty, short } from "@/lib/format";
import { sendIxs, explorerTx } from "@/lib/tx";
import { useMarketMeta } from "@/lib/meta";
import { Button, Badge } from "./ui";
import { useToast } from "./Toast";

const tag: Record<number, { label: string; tone: "yes" | "no" | "neutral" }> = {
  [OUTCOME.YES_WON]: { label: "YES won", tone: "yes" },
  [OUTCOME.NO_WON]: { label: "NO won", tone: "no" },
  [OUTCOME.INVALID]: { label: "Invalid · refund", tone: "neutral" },
};

export function PortfolioRow({ holding, onChange }: { holding: Holding; onChange: () => void }) {
  const { connection } = useConnection();
  const wallet = useWallet();
  const { push } = useToast();
  const [busy, setBusy] = useState(false);

  const { market, position, address, marketKey, resolved, claimable } = holding;
  const label = useMarketMeta().getLabel(address);
  const title = label?.title ?? `Market ${short(address, 5)}`;

  async function claim() {
    if (!wallet.publicKey) return;
    setBusy(true);
    try {
      const mintKey = new PublicKey(market.collateralMint);
      const vault = new PublicKey(market.vault);
      const ata = userAta(mintKey, wallet.publicKey);
      const ixs: TransactionInstruction[] = [];
      const { ix: createIx } = await maybeCreateAta(connection, wallet.publicKey, mintKey, wallet.publicKey);
      if (createIx) ixs.push(createIx);
      ixs.push(ixClaim(wallet.publicKey, marketKey, ata, vault, mintKey));
      const sig = await sendIxs(connection, wallet, ixs, wallet.publicKey);
      push({ tone: "ok", msg: `Claimed ${usd(claimable)}.`, href: explorerTx(sig) });
      onChange();
    } catch (e: any) {
      push({ tone: "err", msg: e?.message ?? "Claim failed." });
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex flex-wrap items-center justify-between gap-4 rounded-xl border border-line bg-panel2/50 px-4 py-3">
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <Link href={`/market/${address}`} className="truncate text-sm font-semibold text-ink hover:text-brand2">
            {title}
          </Link>
          {resolved && tag[market.outcome] && <Badge tone={tag[market.outcome].tone}>{tag[market.outcome].label}</Badge>}
        </div>
        <div className="mt-0.5 flex flex-wrap gap-x-4 text-[11px] text-muted">
          <span className="text-yes">YES {qty(position.yesQty)}</span>
          <span className="text-no">NO {qty(position.noQty)}</span>
          <span>collateral {usd(position.collateral)}</span>
        </div>
      </div>

      {resolved ? (
        claimable > 0n ? (
          <div className="flex items-center gap-3">
            <div className="text-right">
              <div className="text-[10px] uppercase tracking-wider text-muted">payout</div>
              <div className="font-mono text-sm font-bold text-brand2">{usd(claimable)}</div>
            </div>
            <Button onClick={claim} loading={busy} disabled={!wallet.publicKey}>
              Claim
            </Button>
          </div>
        ) : (
          <span className="text-xs text-muted">Settled · nothing to claim</span>
        )
      ) : (
        <Link href={`/market/${address}`} className="text-xs font-medium text-brand2 hover:underline">
          Trade →
        </Link>
      )}
    </div>
  );
}
