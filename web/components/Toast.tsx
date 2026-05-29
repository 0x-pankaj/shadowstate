"use client";

import { createContext, ReactNode, useCallback, useContext, useState } from "react";

type Tone = "ok" | "err" | "info";
interface Toast {
  id: number;
  tone: Tone;
  msg: string;
  href?: string;
}

const Ctx = createContext<{ push: (t: Omit<Toast, "id">) => void }>({ push: () => {} });

export function useToast() {
  return useContext(Ctx);
}

let seq = 1;

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);

  const push = useCallback((t: Omit<Toast, "id">) => {
    const id = seq++;
    setToasts((xs) => [...xs, { ...t, id }]);
    setTimeout(() => setToasts((xs) => xs.filter((x) => x.id !== id)), 7000);
  }, []);

  return (
    <Ctx.Provider value={{ push }}>
      {children}
      <div className="pointer-events-none fixed bottom-5 right-5 z-50 flex w-[360px] max-w-[calc(100vw-2.5rem)] flex-col gap-2">
        {toasts.map((t) => (
          <div
            key={t.id}
            className={`pointer-events-auto rounded-xl border px-4 py-3 text-sm shadow-glow backdrop-blur-md ${
              t.tone === "ok"
                ? "border-yes/40 bg-yes/10 text-yes"
                : t.tone === "err"
                  ? "border-no/40 bg-no/10 text-no"
                  : "border-brand/40 bg-brand/10 text-brand2"
            }`}
          >
            <div className="break-words">{t.msg}</div>
            {t.href && (
              <a href={t.href} target="_blank" rel="noreferrer" className="mt-1 inline-block underline opacity-80 hover:opacity-100">
                View on explorer ↗
              </a>
            )}
          </div>
        ))}
      </div>
    </Ctx.Provider>
  );
}
