"use client";

import { useCallback, useEffect, useState } from "react";
import { Connection, PublicKey } from "@solana/web3.js";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import { decodeMarket, decodePosition, MarketState, UserPosition } from "./state";
import { positionPda } from "./pdas";
import { userAta, tokenBalance } from "./token";

export interface MarketSnap {
  loading: boolean;
  error: string | null;
  market: MarketState | null;
  marketKey: PublicKey | null;
  mintKey: PublicKey | null;
  position: UserPosition | null;
  positionExists: boolean;
  sol: number;
  collateralBalance: bigint;
}

const empty: MarketSnap = {
  loading: false,
  error: null,
  market: null,
  marketKey: null,
  mintKey: null,
  position: null,
  positionExists: false,
  sol: 0,
  collateralBalance: 0n,
};

async function load(conn: Connection, address: string, owner: PublicKey | null): Promise<MarketSnap> {
  let marketKey: PublicKey;
  try {
    marketKey = new PublicKey(address);
  } catch {
    return { ...empty, error: "Invalid market address." };
  }

  const info = await conn.getAccountInfo(marketKey);
  const market = info ? decodeMarket(info.data) : null;
  if (!market) return { ...empty, marketKey, error: "Market account not found on this RPC." };

  // The collateral mint is per-market — read it straight from on-chain state.
  const mintKey = new PublicKey(market.collateralMint);

  let position: UserPosition | null = null;
  let positionExists = false;
  let sol = 0;
  let collateralBalance = 0n;

  if (owner) {
    const [posInfo, lamports, bal] = await Promise.all([
      conn.getAccountInfo(positionPda(marketKey, owner)),
      conn.getBalance(owner),
      tokenBalance(conn, userAta(mintKey, owner)),
    ]);
    positionExists = !!posInfo;
    position = posInfo ? decodePosition(posInfo.data) : null;
    sol = lamports / 1e9;
    collateralBalance = bal;
  }

  return { loading: false, error: null, market, marketKey, mintKey, position, positionExists, sol, collateralBalance };
}

/** Load one market + the connected user's position/balances for it. */
export function useMarket(address: string): MarketSnap & { refresh: () => void } {
  const { connection } = useConnection();
  const { publicKey } = useWallet();
  const [snap, setSnap] = useState<MarketSnap>({ ...empty, loading: true });

  const refresh = useCallback(() => {
    setSnap((s) => ({ ...s, loading: true }));
    load(connection, address, publicKey ?? null)
      .then(setSnap)
      .catch((e) => setSnap({ ...empty, error: e?.message ?? String(e) }));
  }, [connection, address, publicKey]);

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, 12_000);
    return () => clearInterval(id);
  }, [refresh]);

  return { ...snap, refresh };
}
