"use client";

import dynamic from "next/dynamic";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { Badge } from "./ui";

// Wallet button touches the DOM/localStorage — render client-only to avoid hydration drift.
const WalletButton = dynamic(
  () => import("@solana/wallet-adapter-react-ui").then((m) => m.WalletMultiButton),
  { ssr: false }
);

const NAV = [
  { href: "/", label: "Markets" },
  { href: "/create", label: "Create" },
  { href: "/portfolio", label: "Portfolio" },
];

export function Header({ sol }: { sol?: number }) {
  const path = usePathname();
  return (
    <header className="sticky top-0 z-30 border-b border-line/70 bg-bg/70 backdrop-blur-xl">
      <div className="mx-auto flex max-w-6xl items-center justify-between gap-4 px-5 py-3.5">
        <div className="flex items-center gap-6">
          <Link href="/" className="flex items-center gap-3">
            <div className="grid h-9 w-9 place-items-center rounded-xl bg-brand-grad text-bg shadow-glow">
              <span className="text-base font-black">◈</span>
            </div>
            <div className="leading-tight">
              <div className="flex items-center gap-2">
                <span className="text-[15px] font-bold tracking-tight text-ink">ShadowState</span>
                <Badge tone="brand">
                  <span className="live-dot inline-block h-1.5 w-1.5 rounded-full bg-brand2" />
                  dark pool
                </Badge>
              </div>
              <div className="text-[11px] text-muted">Confidential prediction market · Arcium MPC</div>
            </div>
          </Link>

          <nav className="hidden items-center gap-1 md:flex">
            {NAV.map((n) => {
              const active = n.href === "/" ? path === "/" || path.startsWith("/market") : path.startsWith(n.href);
              return (
                <Link
                  key={n.href}
                  href={n.href}
                  className={`rounded-lg px-3 py-1.5 text-sm font-medium transition ${
                    active ? "bg-panel2 text-ink" : "text-muted hover:text-ink"
                  }`}
                >
                  {n.label}
                </Link>
              );
            })}
          </nav>
        </div>

        <div className="flex items-center gap-3">
          {sol !== undefined && (
            <div className="hidden rounded-xl border border-line bg-panel2/60 px-3 py-2 text-right sm:block">
              <div className="text-[10px] uppercase tracking-wider text-muted">Wallet SOL</div>
              <div className="font-mono text-sm font-semibold tabular-nums text-ink">{sol.toFixed(3)}</div>
            </div>
          )}
          <WalletButton />
        </div>
      </div>
    </header>
  );
}
