"use client";

import { useMemo, useState } from "react";
import { PublicKey } from "@solana/web3.js";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import { SIDE, MIDPOINT } from "@/lib/constants";
import { EPOCH } from "@/lib/env";
import { toBase, usd } from "@/lib/format";
import { explorerTx } from "@/lib/tx";
import { Panel, Button, Input, Badge } from "./ui";
import { useToast } from "./Toast";

export function OrderTicket({ marketKey }: { marketKey: PublicKey | null }) {
  const { connection } = useConnection();
  const wallet = useWallet();
  const { push } = useToast();
  const [side, setSide] = useState<0 | 1>(SIDE.YES as 0);
  const [qtyStr, setQtyStr] = useState("");
  const [busy, setBusy] = useState(false);

  const canSeal = !!wallet.publicKey && !!wallet.signTransaction && !!wallet.signAllTransactions;
  const qtyBase = useMemo(() => toBase(qtyStr), [qtyStr]);
  // Tier-1 indicative cost: every contract crosses P2P at the $0.50 midpoint.
  const indicativeCost = (qtyBase * BigInt(MIDPOINT)) / 1_000_000n;

  async function seal() {
    if (!wallet.publicKey || !marketKey || !canSeal) return;
    if (qtyBase <= 0n) return push({ tone: "err", msg: "Enter a contract size greater than zero." });
    setBusy(true);
    try {
      const { placeSealedOrder } = await import("@/lib/arcium");
      const sig = await placeSealedOrder({
        connection,
        wallet: {
          publicKey: wallet.publicKey,
          signTransaction: wallet.signTransaction!,
          signAllTransactions: wallet.signAllTransactions!,
        },
        market: marketKey,
        epoch: EPOCH,
        side,
        qty: qtyBase,
      });
      push({ tone: "ok", msg: "Sealed order submitted to the MPC batch.", href: explorerTx(sig) });
      setQtyStr("");
    } catch (e: any) {
      push({ tone: "err", msg: e?.message ?? "Failed to seal order." });
    } finally {
      setBusy(false);
    }
  }

  return (
    <Panel
      title="Place sealed order"
      hint="Encrypted client-side · matched off-chain in the Arcium MXE"
      right={<Badge tone="brand">🔒 confidential</Badge>}
    >
      <div className="mb-3 grid grid-cols-2 gap-2">
        <button
          onClick={() => setSide(SIDE.YES as 0)}
          className={`rounded-xl border py-3 text-sm font-bold transition ${
            side === SIDE.YES ? "border-yes bg-yes/15 text-yes" : "border-line text-muted hover:text-ink"
          }`}
        >
          Buy YES
        </button>
        <button
          onClick={() => setSide(SIDE.NO as 1)}
          className={`rounded-xl border py-3 text-sm font-bold transition ${
            side === SIDE.NO ? "border-no bg-no/15 text-no" : "border-line text-muted hover:text-ink"
          }`}
        >
          Buy NO
        </button>
      </div>

      <div className="mb-1.5 text-xs text-muted">Contracts</div>
      <Input
        inputMode="decimal"
        placeholder="0"
        value={qtyStr}
        onChange={(e) => setQtyStr(e.target.value)}
        disabled={!canSeal || busy}
      />

      <div className="mt-3 flex items-center justify-between rounded-xl border border-line bg-panel2/60 px-4 py-2.5 text-sm">
        <span className="text-muted">Indicative cost (Tier-1 @ $0.50)</span>
        <span className="font-mono font-semibold text-ink">{usd(indicativeCost)}</span>
      </div>

      <Button
        className="mt-3 w-full"
        variant={side === SIDE.YES ? "yes" : "no"}
        onClick={seal}
        loading={busy}
        disabled={!canSeal || !marketKey}
      >
        {busy ? "Sealing…" : `Seal ${side === SIDE.YES ? "YES" : "NO"} order`}
      </Button>

      <p className="mt-3 text-[11px] leading-relaxed text-muted">
        Your side and size are encrypted in your browser (x25519 + RescueCipher) before they ever leave this page. The
        MXE aggregates the batch, matches peer-to-peer first, then settles the residual on-chain. Nobody — not the
        relayer, not the chain — sees your order until the epoch clears.
      </p>
      {!canSeal && wallet.publicKey && (
        <p className="mt-2 text-[11px] text-no">This wallet can&apos;t sign transactions for sealing.</p>
      )}
    </Panel>
  );
}
