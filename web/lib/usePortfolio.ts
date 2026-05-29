"use client";

import { useCallback, useEffect, useState } from "react";
import { Connection, PublicKey } from "@solana/web3.js";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import { discoverUserPositions, loadMarkets } from "./discover";
import { MarketState, UserPosition } from "./state";
import { OUTCOME } from "./constants";
import { claimPayout } from "./payout";

export interface Holding {
  marketKey: PublicKey;
  address: string;
  market: MarketState;
  position: UserPosition;
  resolved: boolean;
  /** Base-unit collateral redeemable now (0 for open or losing markets). */
  claimable: bigint;
}

export interface PortfolioState {
  loading: boolean;
  error: string | null;
  /** Resolved markets where the user has winnings/refund to claim. */
  claimable: Holding[];
  /** Markets still trading where the user holds contracts or collateral. */
  open: Holding[];
  /** Resolved markets already settled with nothing to claim (losses). */
  settled: Holding[];
}

async function load(conn: Connection, owner: PublicKey): Promise<PortfolioState> {
  const positions = await discoverUserPositions(conn, owner);
  const marketKeys = positions.map((p) => new PublicKey(p.position.market));
  const markets = await loadMarkets(conn, marketKeys);

  const claimable: Holding[] = [];
  const open: Holding[] = [];
  const settled: Holding[] = [];

  for (const { position } of positions) {
    const marketKey = new PublicKey(position.market);
    const market = markets.get(marketKey.toBase58());
    if (!market) continue;
    const resolved = market.outcome !== OUTCOME.UNRESOLVED;
    const amt = claimPayout(position, market);
    const h: Holding = { marketKey, address: marketKey.toBase58(), market, position, resolved, claimable: amt };

    if (resolved && amt > 0n) claimable.push(h);
    else if (resolved) settled.push(h);
    else if (position.yesQty > 0n || position.noQty > 0n || position.collateral > 0n) open.push(h);
  }

  return { loading: false, error: null, claimable, open, settled };
}

export function usePortfolio(): PortfolioState & { refresh: () => void } {
  const { connection } = useConnection();
  const { publicKey } = useWallet();
  const [state, setState] = useState<PortfolioState>({ loading: true, error: null, claimable: [], open: [], settled: [] });

  const refresh = useCallback(() => {
    if (!publicKey) {
      setState({ loading: false, error: null, claimable: [], open: [], settled: [] });
      return;
    }
    setState((s) => ({ ...s, loading: true }));
    load(connection, publicKey)
      .then(setState)
      .catch((e) =>
        setState({
          loading: false,
          error: e?.message ?? "Couldn't load portfolio (RPC may not support getProgramAccounts).",
          claimable: [],
          open: [],
          settled: [],
        })
      );
  }, [connection, publicKey]);

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, 15_000);
    return () => clearInterval(id);
  }, [refresh]);

  return { ...state, refresh };
}
