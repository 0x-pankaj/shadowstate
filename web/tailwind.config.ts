import type { Config } from "tailwindcss";

const config: Config = {
  content: ["./app/**/*.{ts,tsx}", "./components/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        bg: "#0a0b10",
        panel: "#12141d",
        panel2: "#171a26",
        line: "#232734",
        ink: "#e8eaf0",
        muted: "#8a90a3",
        yes: "#22d3a0",
        no: "#f4587a",
        brand: "#7c6cff",
        brand2: "#46e6c8",
      },
      fontFamily: {
        sans: ["ui-sans-serif", "system-ui", "Inter", "-apple-system", "Segoe UI", "sans-serif"],
        mono: ["ui-monospace", "SFMono-Regular", "Menlo", "monospace"],
      },
      boxShadow: {
        glow: "0 0 0 1px rgba(124,108,255,0.25), 0 8px 40px -12px rgba(124,108,255,0.45)",
      },
      backgroundImage: {
        "brand-grad": "linear-gradient(135deg,#7c6cff 0%,#46e6c8 100%)",
      },
    },
  },
  plugins: [],
};
export default config;
