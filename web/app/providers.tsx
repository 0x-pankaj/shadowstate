"use client";

import { ReactNode, useCallback, useMemo } from "react";
import { ConnectionProvider, WalletProvider } from "@solana/wallet-adapter-react";
import { WalletModalProvider } from "@solana/wallet-adapter-react-ui";
import { PhantomWalletAdapter, SolflareWalletAdapter } from "@solana/wallet-adapter-wallets";
import { WalletConnectionError, WalletError, WalletNotReadyError } from "@solana/wallet-adapter-base";
import { RPC_URL } from "@/lib/constants";
import { ToastProvider } from "@/components/Toast";
import { MarketMetaProvider } from "@/lib/meta";

import "@solana/wallet-adapter-react-ui/styles.css";

export function Providers({ children }: { children: ReactNode }) {
  // Phantom/Solflare also register via the Wallet Standard; wallet-adapter dedupes them by name,
  // so listing them here is safe and just guarantees they appear even on first load.
  const wallets = useMemo(() => [new PhantomWalletAdapter(), new SolflareWalletAdapter()], []);

  // `autoConnect` tries to reconnect every registered wallet on load — including the Mobile
  // Wallet Adapter that ships transitively with wallet-adapter-react, which rejects on desktop
  // with an empty WalletConnectionError ("Unexpected error"). Without a handler that rejection
  // is uncaught and blocks connecting. Ignore those expected autoConnect failures; log the rest.
  const onError = useCallback((err: WalletError) => {
    if (err instanceof WalletConnectionError || err instanceof WalletNotReadyError) return;
    console.warn("[wallet]", err?.name, err?.message);
  }, []);

  return (
    <ConnectionProvider endpoint={RPC_URL} config={{ commitment: "confirmed" }}>
      <WalletProvider wallets={wallets} autoConnect onError={onError}>
        <WalletModalProvider>
          <ToastProvider>
            <MarketMetaProvider>{children}</MarketMetaProvider>
          </ToastProvider>
        </WalletModalProvider>
      </WalletProvider>
    </ConnectionProvider>
  );
}
