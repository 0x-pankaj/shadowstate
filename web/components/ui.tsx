"use client";

import { ButtonHTMLAttributes, InputHTMLAttributes, ReactNode } from "react";

export function Panel({
  children,
  className = "",
  title,
  hint,
  right,
}: {
  children: ReactNode;
  className?: string;
  title?: string;
  hint?: string;
  right?: ReactNode;
}) {
  return (
    <section
      className={`rounded-2xl border border-line bg-panel/80 backdrop-blur-sm p-5 shadow-[0_8px_40px_-24px_rgba(0,0,0,0.8)] ${className}`}
    >
      {(title || right) && (
        <header className="mb-4 flex items-start justify-between gap-3">
          <div>
            {title && <h2 className="text-sm font-semibold tracking-wide text-ink">{title}</h2>}
            {hint && <p className="mt-0.5 text-xs text-muted">{hint}</p>}
          </div>
          {right}
        </header>
      )}
      {children}
    </section>
  );
}

export function Stat({ label, value, accent }: { label: string; value: ReactNode; accent?: "yes" | "no" | "brand" }) {
  const color = accent === "yes" ? "text-yes" : accent === "no" ? "text-no" : accent === "brand" ? "text-brand2" : "text-ink";
  return (
    <div className="rounded-xl border border-line bg-panel2/60 px-4 py-3">
      <div className="text-[11px] uppercase tracking-wider text-muted">{label}</div>
      <div className={`mt-1 font-mono text-lg font-semibold tabular-nums ${color}`}>{value}</div>
    </div>
  );
}

type BtnProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "yes" | "no" | "ghost";
  loading?: boolean;
};

export function Button({ variant = "primary", loading, className = "", children, disabled, ...rest }: BtnProps) {
  const base =
    "inline-flex items-center justify-center gap-2 rounded-xl px-4 py-2.5 text-sm font-semibold transition disabled:cursor-not-allowed disabled:opacity-40";
  const styles: Record<string, string> = {
    primary: "bg-brand-grad text-bg hover:brightness-110",
    yes: "bg-yes/15 text-yes border border-yes/40 hover:bg-yes/25",
    no: "bg-no/15 text-no border border-no/40 hover:bg-no/25",
    ghost: "border border-line text-ink hover:bg-panel2",
  };
  return (
    <button className={`${base} ${styles[variant]} ${className}`} disabled={disabled || loading} {...rest}>
      {loading && <Spinner />}
      {children}
    </button>
  );
}

export function Input({ className = "", ...rest }: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      className={`w-full rounded-xl border border-line bg-panel2 px-3 py-2.5 font-mono text-sm text-ink outline-none placeholder:text-muted/60 focus:border-brand/60 ${className}`}
      {...rest}
    />
  );
}

export function Spinner() {
  return (
    <span className="inline-block h-3.5 w-3.5 animate-spin rounded-full border-2 border-current border-t-transparent" />
  );
}

export function Badge({ children, tone = "neutral" }: { children: ReactNode; tone?: "neutral" | "yes" | "no" | "brand" }) {
  const tones: Record<string, string> = {
    neutral: "border-line text-muted",
    yes: "border-yes/40 text-yes bg-yes/10",
    no: "border-no/40 text-no bg-no/10",
    brand: "border-brand/40 text-brand2 bg-brand/10",
  };
  return (
    <span className={`inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-[11px] font-medium ${tones[tone]}`}>
      {children}
    </span>
  );
}
