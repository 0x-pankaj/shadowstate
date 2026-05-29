"use client";

import { useState } from "react";
import { PublicKey } from "@solana/web3.js";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import { UserPosition, MarketState } from "@/lib/state";
import { OUTCOME } from "@/lib/constants";
import { usd, qty } from "@/lib/format";
import { ixInitPosition } from "@/lib/ix";
import { claimPayout } from "@/lib/payout";
import { sendIxs, explorerTx } from "@/lib/tx";
import { Panel, Stat, Button, Badge } from "./ui";
import { useToast } from "./Toast";

export function Portfolio({
  market,
  marketKey,
  position,
  positionExists,
  onChange,
}: {
  market: MarketState | null;
  marketKey: PublicKey | null;
  position: UserPosition | null;
  positionExists: boolean;
  onChange: () => void;
}) {
  const { connection } = useConnection();
  const wallet = useWallet();
  const { push } = useToast();
  const [busy, setBusy] = useState(false);

  const connected = !!wallet.publicKey;

  async function initPosition() {
    if (!wallet.publicKey || !marketKey) return;
    setBusy(true);
    try {
      const ix = ixInitPosition(wallet.publicKey, wallet.publicKey, marketKey);
      const sig = await sendIxs(connection, wallet, [ix], wallet.publicKey);
      push({ tone: "ok", msg: "Position account created.", href: explorerTx(sig) });
      onChange();
    } catch (e: any) {
      push({ tone: "err", msg: e?.message ?? "Failed to create position." });
    } finally {
      setBusy(false);
    }
  }

  const resolved = market && market.outcome !== OUTCOME.UNRESOLVED;
  const claimable = position && market ? claimPayout(position, market) : 0n;

  return (
    <Panel
      title="Your portfolio"
      right={
        positionExists ? (
          <Badge tone="brand">active</Badge>
        ) : connected ? (
          <Badge tone="neutral">no position</Badge>
        ) : null
      }
    >
      {!connected ? (
        <p className="text-sm text-muted">Connect a wallet to view your positions.</p>
      ) : !positionExists ? (
        <div className="flex flex-col items-start gap-3">
          <p className="text-sm text-muted">
            You don&apos;t have a position account for this market yet. Create one to deposit collateral and trade.
          </p>
          <Button onClick={initPosition} loading={busy}>
            Initialize position
          </Button>
        </div>
      ) : (
        <>
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
            <Stat label="YES contracts" value={qty(position?.yesQty ?? 0n)} accent="yes" />
            <Stat label="NO contracts" value={qty(position?.noQty ?? 0n)} accent="no" />
            <Stat label="Collateral" value={usd(position?.collateral ?? 0n)} accent="brand" />
          </div>
          {resolved && (
            <div className="mt-3 rounded-xl border border-brand/30 bg-brand/5 px-4 py-3 text-sm">
              <span className="text-muted">Claimable at settlement: </span>
              <span className="font-mono font-semibold text-brand2">{usd(claimable)}</span>
              <span className="ml-1 text-xs text-muted">
                ({market!.outcome === OUTCOME.INVALID ? "voided · each leg × $0.50" : "winning contracts × $1"})
              </span>
            </div>
          )}
        </>
      )}
    </Panel>
  );
}
