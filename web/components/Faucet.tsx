"use client";

import { useState } from "react";
import { PublicKey } from "@solana/web3.js";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import { faucetEnabled, isFaucetMint, mintTestTokens, FAUCET_MINT } from "@/lib/faucet";
import { toBase, usd, short } from "@/lib/format";
import { explorerTx } from "@/lib/tx";
import { Panel, Button, Input, Badge } from "./ui";
import { useToast } from "./Toast";

export function Faucet({ mint, onChange }: { mint: PublicKey | null; onChange: () => void }) {
  const { connection } = useConnection();
  const wallet = useWallet();
  const { push } = useToast();
  const [amount, setAmount] = useState("1000");
  const [busy, setBusy] = useState(false);

  if (!faucetEnabled()) return null;

  const matches = isFaucetMint(mint);

  async function mint_() {
    if (!wallet.publicKey) return;
    const base = toBase(amount);
    if (base <= 0n) return push({ tone: "err", msg: "Enter an amount greater than zero." });
    setBusy(true);
    try {
      const sig = await mintTestTokens(connection, wallet, base);
      push({ tone: "ok", msg: `Minted ${usd(base)} test USDC to your wallet.`, href: explorerTx(sig) });
      onChange();
    } catch (e: any) {
      push({ tone: "err", msg: e?.message ?? "Mint failed." });
    } finally {
      setBusy(false);
    }
  }

  return (
    <Panel
      title="Test USDC faucet"
      hint="Devnet only · mint as much as you want, then deposit it as collateral"
      right={<Badge tone="brand">🚰 devnet</Badge>}
    >
      <div className="mb-3 flex items-center justify-between rounded-xl border border-line bg-panel2/60 px-4 py-2.5 text-xs">
        <span className="text-muted">Mint</span>
        <a
          href={`https://explorer.solana.com/address/${FAUCET_MINT}?cluster=devnet`}
          target="_blank"
          rel="noreferrer"
          className="font-mono text-ink hover:text-brand2"
        >
          {short(FAUCET_MINT, 6)} ↗
        </a>
      </div>

      {!matches && mint && (
        <p className="mb-3 text-[11px] text-no">
          Heads up: this market&apos;s collateral isn&apos;t the faucet token, so minting here won&apos;t fund it.
        </p>
      )}

      <div className="flex gap-2">
        <Input
          inputMode="decimal"
          value={amount}
          onChange={(e) => setAmount(e.target.value)}
          disabled={!wallet.publicKey || busy}
        />
        <Button onClick={mint_} loading={busy} disabled={!wallet.publicKey} className="shrink-0">
          Mint test USDC
        </Button>
      </div>
      <p className="mt-2 text-[11px] text-muted">
        Real devnet USDC can&apos;t be freely minted, so we run our own test mint. These tokens have no value.
      </p>
    </Panel>
  );
}
