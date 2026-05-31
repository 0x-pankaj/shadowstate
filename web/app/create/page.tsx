"use client";

import Link from "next/link";
import { Header } from "@/components/Header";
import { CreateMarketForm } from "@/components/CreateMarketForm";

export default function CreatePage() {
  return (
    <main className="min-h-screen">
      <Header />
      <div className="mx-auto max-w-2xl px-5 py-7">
        <div className="mb-5">
          <Link href="/" className="text-xs text-muted hover:text-ink">
            ← All markets
          </Link>
          <h1 className="mt-1 text-2xl font-black tracking-tight text-ink">New market</h1>
          <p className="mt-1 text-sm text-muted">
            Anyone can launch a market — the creator becomes its authority (resolves the outcome and tunes risk params).
          </p>
        </div>
        <CreateMarketForm />
      </div>
    </main>
  );
}
