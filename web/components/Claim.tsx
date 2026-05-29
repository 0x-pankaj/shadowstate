"use client";

import { useState } from "react";
import { PublicKey, TransactionInstruction } from "@solana/web3.js";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import { MarketState, UserPosition } from "@/lib/state";
import { OUTCOME } from "@/lib/constants";
import { ixClaim } from "@/lib/ix";
import { claimPayout } from "@/lib/payout";
import { userAta, maybeCreateAta } from "@/lib/token";
import { usd } from "@/lib/format";
import { sendIxs, explorerTx } from "@/lib/tx";
import { Panel, Button, Badge } from "./ui";
import { useToast } from "./Toast";

const outcomeLabel: Record<number, string> = {
  [OUTCOME.YES_WON]: "YES won",
  [OUTCOME.NO_WON]: "NO won",
  [OUTCOME.INVALID]: "Invalid — settled at $0.50",
};

export function Claim({
  market,
  marketKey,
  position,
  onChange,
}: {
  market: MarketState | null;
  marketKey: PublicKey | null;
  position: UserPosition | null;
  onChange: () => void;
}) {
  const { connection } = useConnection();
  const wallet = useWallet();
  const { push } = useToast();
  const [busy, setBusy] = useState(false);

  const resolved = !!market && market.outcome !== OUTCOME.UNRESOLVED;
  const payout = position && market ? claimPayout(position, market) : 0n;

  async function claim() {
    if (!wallet.publicKey || !marketKey || !market) return;
    setBusy(true);
    try {
      const mintKey = new PublicKey(market.collateralMint);
      const ata = userAta(mintKey, wallet.publicKey);
      const vault = new PublicKey(market.vault);
      const ixs: TransactionInstruction[] = [];
      const { ix: createIx } = await maybeCreateAta(connection, wallet.publicKey, mintKey, wallet.publicKey);
      if (createIx) ixs.push(createIx);
      ixs.push(ixClaim(wallet.publicKey, marketKey, ata, vault, mintKey));
      const sig = await sendIxs(connection, wallet, ixs, wallet.publicKey);
      push({ tone: "ok", msg: `Claimed ${usd(payout)}.`, href: explorerTx(sig) });
      onChange();
    } catch (e: any) {
      push({ tone: "err", msg: e?.message ?? "Claim failed." });
    } finally {
      setBusy(false);
    }
  }

  return (
    <Panel
      title="Settlement & claim"
      right={resolved ? <Badge tone="brand">{outcomeLabel[market!.outcome]}</Badge> : <Badge tone="neutral">unresolved</Badge>}
    >
      {!resolved ? (
        <p className="text-sm text-muted">
          This market is still trading. Once the committee resolves the outcome, winning contracts redeem for $1 each here.
        </p>
      ) : (
        <div className="flex flex-col items-start gap-3">
          <div className="rounded-xl border border-brand/30 bg-brand/5 px-4 py-3">
            <div className="text-[11px] uppercase tracking-wider text-muted">Your payout</div>
            <div className="font-mono text-2xl font-bold tabular-nums text-brand2">{usd(payout)}</div>
          </div>
          <Button onClick={claim} loading={busy} disabled={!wallet.publicKey || payout <= 0n}>
            Claim winnings
          </Button>
          {payout <= 0n && wallet.publicKey && (
            <p className="text-xs text-muted">Nothing to claim for this outcome.</p>
          )}
        </div>
      )}
    </Panel>
  );
}
