"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";
import { PublicKey, TransactionInstruction } from "@solana/web3.js";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import {
  TOKEN_2022_PROGRAM_ID,
  ASSOCIATED_TOKEN_PROGRAM_ID,
  createAssociatedTokenAccountIdempotentInstruction,
} from "@solana/spl-token";
import { SCALE } from "@/lib/constants";
import { marketPda, vaultAuthorityPda } from "@/lib/pdas";
import { ixInitializeMarket } from "@/lib/ix";
import { userAta } from "@/lib/token";
import { saveMarketLabel } from "@/lib/markets";
import { pinMarketMeta, useMarketMeta } from "@/lib/meta";
import { FAUCET_MINT } from "@/lib/faucet";
import { sendIxs, explorerTx } from "@/lib/tx";
import { Panel, Button, Input, Badge } from "./ui";
import { useToast } from "./Toast";

const MIN_PRICE = 10_000; // $0.01
const MAX_PRICE = 990_000; // $0.99

export function CreateMarketForm() {
  const { connection } = useConnection();
  const wallet = useWallet();
  const { push } = useToast();
  const { refresh: refreshMeta } = useMarketMeta();
  const router = useRouter();

  const [question, setQuestion] = useState("");
  const [category, setCategory] = useState("");
  const [yesPct, setYesPct] = useState("60");
  const [mintStr, setMintStr] = useState(FAUCET_MINT);
  const [premiumPct, setPremiumPct] = useState("10");
  const [adv, setAdv] = useState(false);
  const [busy, setBusy] = useState(false);

  async function create() {
    if (!wallet.publicKey) return push({ tone: "err", msg: "Connect a wallet first." });
    let mint: PublicKey;
    try {
      mint = new PublicKey(mintStr.trim());
    } catch {
      return push({ tone: "err", msg: "Collateral mint is not a valid address." });
    }
    const yes = Math.round(parseFloat(yesPct) * 10_000); // % → 6-dec price
    if (!isFinite(yes) || yes < MIN_PRICE || yes > MAX_PRICE) {
      return push({ tone: "err", msg: "YES probability must be between 1% and 99%." });
    }
    const premium = Math.round(parseFloat(premiumPct) * 10_000);
    if (!isFinite(premium) || premium < 0 || premium > MAX_PRICE) {
      return push({ tone: "err", msg: "Premium must be between 0% and 99%." });
    }

    setBusy(true);
    try {
      const authority = wallet.publicKey;
      const market = marketPda(authority);
      const vaultAuth = vaultAuthorityPda(market);
      const vault = userAta(mint, vaultAuth); // vault token account owned by the vault PDA
      const mmAccount = userAta(mint, authority); // MM fee account (operator)

      const existing = await connection.getAccountInfo(market);
      if (existing) {
        push({ tone: "info", msg: "You already have a market for this wallet — opening it." });
        router.push(`/market/${market.toBase58()}`);
        return;
      }

      const ixs: TransactionInstruction[] = [
        createAssociatedTokenAccountIdempotentInstruction(
          authority, vault, vaultAuth, mint, TOKEN_2022_PROGRAM_ID, ASSOCIATED_TOKEN_PROGRAM_ID
        ),
        createAssociatedTokenAccountIdempotentInstruction(
          authority, mmAccount, authority, mint, TOKEN_2022_PROGRAM_ID, ASSOCIATED_TOKEN_PROGRAM_ID
        ),
        ixInitializeMarket(authority, authority, mint, vault, mmAccount, {
          baseOraclePrice: BigInt(yes),
          maxSkewPremium: BigInt(premium),
          imbalanceThreshold: BigInt(SCALE), // 1.0 — net imbalance at which skew saturates
          members: [authority], // single-member committee (operator)
          threshold: 1,
          settlementAuthority: authority, // enable the trusted gateway/relayer settle path
        }),
      ];

      const sig = await sendIxs(connection, wallet, ixs, authority);
      const title = question.trim();
      if (title) {
        // Local copy for instant display; pin to Pinata so every visitor sees the title too.
        saveMarketLabel(market.toBase58(), { title, category: category.trim() || undefined });
        pinMarketMeta(market.toBase58(), title, category.trim() || undefined)
          .then(() => refreshMeta())
          .catch((e) => push({ tone: "info", msg: `Market live; title not pinned (${e.message}).` }));
      }
      push({ tone: "ok", msg: "Market created.", href: explorerTx(sig) });
      router.push(`/market/${market.toBase58()}`);
    } catch (e: any) {
      push({ tone: "err", msg: e?.message ?? "Failed to create market." });
    } finally {
      setBusy(false);
    }
  }

  const yesNum = Math.max(0, Math.min(100, parseFloat(yesPct) || 0));

  return (
    <Panel title="Create a market" hint="You become the market authority (resolver + risk admin)." right={<Badge tone="brand">operator</Badge>}>
      <div className="flex flex-col gap-4">
        <div>
          <label className="mb-1.5 block text-xs text-muted">Question</label>
          <Input
            placeholder="Will ETH close above $4,000 on Jun 30?"
            value={question}
            onChange={(e) => setQuestion(e.target.value)}
            disabled={busy}
            className="font-sans"
          />
        </div>

        <div className="grid gap-4 sm:grid-cols-2">
          <div>
            <label className="mb-1.5 block text-xs text-muted">Category (optional)</label>
            <Input placeholder="Crypto" value={category} onChange={(e) => setCategory(e.target.value)} disabled={busy} className="font-sans" />
          </div>
          <div>
            <label className="mb-1.5 flex justify-between text-xs text-muted">
              <span>Starting YES odds</span>
              <span className="text-yes">{yesNum.toFixed(0)}¢ · NO {(100 - yesNum).toFixed(0)}¢</span>
            </label>
            <Input inputMode="decimal" value={yesPct} onChange={(e) => setYesPct(e.target.value)} disabled={busy} />
          </div>
        </div>

        <div>
          <label className="mb-1.5 block text-xs text-muted">Collateral mint (Token-2022)</label>
          <Input value={mintStr} onChange={(e) => setMintStr(e.target.value)} disabled={busy} />
          <p className="mt-1 text-[11px] text-muted">Defaults to your test-USDC faucet mint.</p>
        </div>

        <button onClick={() => setAdv((a) => !a)} className="self-start text-xs text-brand2 hover:underline">
          {adv ? "− Hide" : "+ Advanced"} risk params
        </button>
        {adv && (
          <div>
            <label className="mb-1.5 block text-xs text-muted">Max skew premium (Tier-2 PropAMM)</label>
            <Input inputMode="decimal" value={premiumPct} onChange={(e) => setPremiumPct(e.target.value)} disabled={busy} />
            <p className="mt-1 text-[11px] text-muted">Extra cents added to the heavy side at full imbalance.</p>
          </div>
        )}

        <Button onClick={create} loading={busy} disabled={!wallet.publicKey} className="mt-1">
          {wallet.publicKey ? "Create market" : "Connect wallet to create"}
        </Button>
        <p className="text-[11px] text-muted">
          The market PDA is derived from your wallet (<span className="font-mono">[b&quot;market&quot;, you]</span>), so one market
          per wallet. It opens in <em>Trading</em> immediately.
        </p>
      </div>
    </Panel>
  );
}
