import { SCALE } from "./constants";

/** 6-dec fixed point → human dollars string. */
export function usd(v: bigint | number, dp = 2): string {
  const n = Number(v) / SCALE;
  return `$${n.toLocaleString(undefined, { minimumFractionDigits: dp, maximumFractionDigits: dp })}`;
}

/** A 6-dec price (e.g. 520_000) → "0.52". */
export function price(v: bigint | number): string {
  return (Number(v) / SCALE).toFixed(2);
}

/** Contract quantity (6-dec base units) → integer-ish display. */
export function qty(v: bigint | number): string {
  return Number(v).toLocaleString();
}

/** Shorten a base58 key. */
export function short(addr: string, n = 4): string {
  return addr.length <= n * 2 + 1 ? addr : `${addr.slice(0, n)}…${addr.slice(-n)}`;
}

/** Parse a user-typed dollar/qty string into 6-dec base units. */
export function toBase(input: string): bigint {
  const f = parseFloat(input);
  if (!isFinite(f) || f < 0) return 0n;
  return BigInt(Math.round(f * SCALE));
}
