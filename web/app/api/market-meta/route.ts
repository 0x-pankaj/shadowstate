import { NextRequest, NextResponse } from "next/server";

// Server-only Pinata access. The JWT NEVER reaches the browser — all pin/list goes through here.
const JWT = process.env.PINATA_JWT || "";
const PROGRAM = process.env.NEXT_PUBLIC_SETTLEMENT_PROGRAM_ID || "FP8riDGv8jrif5G8QfprfzPeNR8kQ3UUrpTp6EXByDVZ";

export const dynamic = "force-dynamic";

/** GET /api/market-meta            → all markets' metadata for this program
 *  GET /api/market-meta?market=PDA → one market's metadata                  */
export async function GET(req: NextRequest) {
  if (!JWT) return NextResponse.json({ markets: {} });
  const market = req.nextUrl.searchParams.get("market");

  const params = new URLSearchParams({ status: "pinned", pageLimit: "1000" });
  params.set("metadata[keyvalues][program]", JSON.stringify({ value: PROGRAM, op: "eq" }));
  if (market) params.set("metadata[keyvalues][market]", JSON.stringify({ value: market, op: "eq" }));

  try {
    const res = await fetch(`https://api.pinata.cloud/data/pinList?${params}`, {
      headers: { Authorization: `Bearer ${JWT}` },
      cache: "no-store",
    });
    if (!res.ok) return NextResponse.json({ markets: {}, error: `pinList ${res.status}` });
    const data = await res.json();
    // Newest pin per market wins (rows come newest-first).
    const markets: Record<string, { title: string; category?: string; cid: string }> = {};
    for (const row of data.rows ?? []) {
      const kv = row.metadata?.keyvalues ?? {};
      if (kv.market && !markets[kv.market]) {
        markets[kv.market] = { title: kv.title || "", category: kv.category || undefined, cid: row.ipfs_pin_hash };
      }
    }
    return NextResponse.json({ markets });
  } catch (e: any) {
    return NextResponse.json({ markets: {}, error: e?.message ?? "fetch failed" });
  }
}

/** POST { market, title, category } → pins metadata JSON keyed by the market PDA. */
export async function POST(req: NextRequest) {
  if (!JWT) return NextResponse.json({ error: "PINATA_JWT not configured" }, { status: 501 });
  const { market, title, category } = await req.json().catch(() => ({}));
  if (!market || !title) return NextResponse.json({ error: "market and title required" }, { status: 400 });

  const body = {
    pinataMetadata: {
      name: `ssmarket:${market}`,
      keyvalues: { market, program: PROGRAM, title: String(title).slice(0, 200), category: category ? String(category).slice(0, 60) : "" },
    },
    pinataContent: { market, title, category: category || null, program: PROGRAM, kind: "shadowstate-market", v: 1 },
  };

  try {
    const res = await fetch("https://api.pinata.cloud/pinning/pinJSONToIPFS", {
      method: "POST",
      headers: { Authorization: `Bearer ${JWT}`, "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    const data = await res.json();
    if (!res.ok) return NextResponse.json({ error: data?.error || `pin ${res.status}` }, { status: 502 });
    return NextResponse.json({ cid: data.IpfsHash });
  } catch (e: any) {
    return NextResponse.json({ error: e?.message ?? "pin failed" }, { status: 502 });
  }
}
