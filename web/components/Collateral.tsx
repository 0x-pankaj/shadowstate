"use client";

import { useState } from "react";
import { PublicKey, TransactionInstruction } from "@solana/web3.js";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import { MarketState } from "@/lib/state";
import { ixDeposit, ixWithdraw } from "@/lib/ix";
import { userAta, maybeCreateAta } from "@/lib/token";
import { toBase, usd } from "@/lib/format";
import { sendIxs, explorerTx } from "@/lib/tx";
import { Panel, Button, Input } from "./ui";
import { useToast } from "./Toast";

type Mode = "deposit" | "withdraw";

export function Collateral({
  market,
  marketKey,
  walletBalance,
  positionExists,
  onChange,
}: {
  market: MarketState | null;
  marketKey: PublicKey | null;
  walletBalance: bigint;
  positionExists: boolean;
  onChange: () => void;
}) {
  const { connection } = useConnection();
  const wallet = useWallet();
  const { push } = useToast();
  const [mode, setMode] = useState<Mode>("deposit");
  const [amount, setAmount] = useState("");
  const [busy, setBusy] = useState(false);

  const disabled = !wallet.publicKey || !marketKey || !market || !positionExists;

  async function submit() {
    if (!wallet.publicKey || !marketKey || !market) return;
    const base = toBase(amount);
    if (base <= 0n) return push({ tone: "err", msg: "Enter an amount greater than zero." });
    setBusy(true);
    try {
      const mintKey = new PublicKey(market.collateralMint);
      const ata = userAta(mintKey, wallet.publicKey);
      const vault = new PublicKey(market.vault);
      const ixs: TransactionInstruction[] = [];

      // Ensure the user's Token-2022 ATA exists (needed as the withdraw destination).
      const { ix: createIx } = await maybeCreateAta(connection, wallet.publicKey, mintKey, wallet.publicKey);
      if (createIx) ixs.push(createIx);

      ixs.push(
        mode === "deposit"
          ? ixDeposit(wallet.publicKey, marketKey, ata, vault, mintKey, base)
          : ixWithdraw(wallet.publicKey, marketKey, ata, vault, mintKey, base)
      );

      const sig = await sendIxs(connection, wallet, ixs, wallet.publicKey);
      push({ tone: "ok", msg: `${mode === "deposit" ? "Deposited" : "Withdrew"} ${usd(base)}.`, href: explorerTx(sig) });
      setAmount("");
      onChange();
    } catch (e: any) {
      push({ tone: "err", msg: e?.message ?? "Transaction failed." });
    } finally {
      setBusy(false);
    }
  }

  return (
    <Panel title="Collateral" hint="Token-2022 vault · $1 backs each contract">
      <div className="mb-3 grid grid-cols-2 gap-1 rounded-xl border border-line bg-panel2 p-1">
        {(["deposit", "withdraw"] as Mode[]).map((m) => (
          <button
            key={m}
            onClick={() => setMode(m)}
            className={`rounded-lg py-2 text-sm font-semibold capitalize transition ${
              mode === m ? "bg-brand-grad text-bg" : "text-muted hover:text-ink"
            }`}
          >
            {m}
          </button>
        ))}
      </div>

      <div className="mb-1.5 flex items-center justify-between text-xs text-muted">
        <span>Amount (USD)</span>
        {mode === "deposit" && <span>Wallet: {usd(walletBalance)}</span>}
      </div>
      <Input
        inputMode="decimal"
        placeholder="0.00"
        value={amount}
        onChange={(e) => setAmount(e.target.value)}
        disabled={disabled || busy}
      />

      <Button className="mt-3 w-full" onClick={submit} loading={busy} disabled={disabled}>
        {mode === "deposit" ? "Deposit collateral" : "Withdraw collateral"}
      </Button>

      {!positionExists && wallet.publicKey && (
        <p className="mt-2 text-xs text-muted">Initialize your position above before depositing.</p>
      )}
    </Panel>
  );
}
