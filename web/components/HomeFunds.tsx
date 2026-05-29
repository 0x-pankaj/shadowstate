"use client";

import { useCallback, useEffect, useState } from "react";
import { PublicKey } from "@solana/web3.js";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import { faucetEnabled, mintTestTokens, FAUCET_MINT } from "@/lib/faucet";
import { userAta, tokenBalance } from "@/lib/token";
import { toBase, usd, short } from "@/lib/format";
import { explorerTx } from "@/lib/tx";
import { Panel, Button, Input, Badge } from "./ui";
import { useToast } from "./Toast";

/** Wallet balances + the global test-USDC faucet — usable before any market exists. */
export function HomeFunds() {
  const { connection } = useConnection();
  const wallet = useWallet();
  const { push } = useToast();
  const [sol, setSol] = useState(0);
  const [usdc, setUsdc] = useState(0n);
  const [amount, setAmount] = useState("1000");
  const [busy, setBusy] = useState(false);

  const enabled = faucetEnabled();

  const refresh = useCallback(async () => {
    if (!wallet.publicKey) {
      setSol(0);
      setUsdc(0n);
      return;
    }
    const [lamports, bal] = await Promise.all([
      connection.getBalance(wallet.publicKey),
      enabled ? tokenBalance(connection, userAta(new PublicKey(FAUCET_MINT), wallet.publicKey)) : Promise.resolve(0n),
    ]);
    setSol(lamports / 1e9);
    setUsdc(bal);
  }, [connection, wallet.publicKey, enabled]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  async function mint() {
    if (!wallet.publicKey) return;
    const base = toBase(amount);
    if (base <= 0n) return push({ tone: "err", msg: "Enter an amount greater than zero." });
    setBusy(true);
    try {
      const sig = await mintTestTokens(connection, wallet, base);
      push({ tone: "ok", msg: `Minted ${usd(base)} test USDC.`, href: explorerTx(sig) });
      refresh();
    } catch (e: any) {
      push({ tone: "err", msg: e?.message ?? "Mint failed." });
    } finally {
      setBusy(false);
    }
  }

  if (!wallet.publicKey) {
    return (
      <Panel title="Your funds" hint="Connect a wallet to mint test USDC and start trading.">
        <p className="text-sm text-muted">Use the Connect button in the top right.</p>
      </Panel>
    );
  }

  return (
    <Panel
      title="Your funds"
      hint="Mint test USDC here, then open a market to deposit it as collateral and trade."
      right={enabled ? <Badge tone="brand">🚰 faucet on</Badge> : <Badge tone="neutral">faucet off</Badge>}
    >
      <div className="grid gap-3 sm:grid-cols-[1fr_1fr_auto] sm:items-end">
        <div className="rounded-xl border border-line bg-panel2/60 px-4 py-3">
          <div className="text-[10px] uppercase tracking-wider text-muted">Wallet SOL</div>
          <div className="font-mono text-lg font-semibold tabular-nums text-ink">{sol.toFixed(3)}</div>
        </div>
        <div className="rounded-xl border border-line bg-panel2/60 px-4 py-3">
          <div className="text-[10px] uppercase tracking-wider text-muted">Test USDC</div>
          <div className="font-mono text-lg font-semibold tabular-nums text-brand2">{usd(usdc)}</div>
        </div>
        {enabled ? (
          <div className="flex gap-2">
            <Input
              inputMode="decimal"
              value={amount}
              onChange={(e) => setAmount(e.target.value)}
              disabled={busy}
              className="w-28"
            />
            <Button onClick={mint} loading={busy} className="shrink-0">
              Mint USDC
            </Button>
          </div>
        ) : (
          <p className="text-[11px] text-muted">
            Faucet not configured — run <code className="text-ink">pnpm faucet:setup</code>.
          </p>
        )}
      </div>

      {enabled && (
        <p className="mt-3 text-[11px] text-muted">
          Mint: <span className="font-mono text-ink">{short(FAUCET_MINT, 6)}</span> · devnet test token, no value. Deposit
          &amp; withdraw happen inside each market.
        </p>
      )}
    </Panel>
  );
}
