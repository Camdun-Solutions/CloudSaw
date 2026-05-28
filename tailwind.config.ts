import type { Config } from "tailwindcss";

const config: Config = {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        saw: {
          red: "#E63946",
          // PR #55: higher-saturation red for primary CTAs. The
          // standard saw-red (#E63946) sits a touch desaturated for
          // body emphasis use; the bold variant pushes saturation +
          // darkens slightly so primary buttons read as the highest-
          // weight element in any view. Contrast vs saw-white text
          // stays >= 4.5:1 (AA).
          "red-bold": "#D52836",
          orange: "#F77F1F",
          gold: "#F2B705",
          // PR #51: "well-configured" / resolved-finding green for
          // the Findings page's color-coded left borders. Picked
          // for AA contrast against saw-white and saw-grey-50
          // backgrounds; passes ratio 4.5:1 for the border-only
          // use case.
          green: "#22A06B",
          black: "#0A0B0D",
          white: "#FFFFFF",
          grey: {
            50: "#F7F8FA",
            100: "#EDEFF3",
            200: "#D9DDE3",
            300: "#B7BDC7",
            400: "#8A92A0",
            500: "#6B7280",
            600: "#4C525C",
            700: "#363B43",
            800: "#23262C",
            900: "#14161A",
          },
        },
      },
      fontFamily: {
        sans: [
          "Inter",
          "system-ui",
          "-apple-system",
          "Segoe UI",
          "Roboto",
          "sans-serif",
        ],
        mono: ["JetBrains Mono", "ui-monospace", "SFMono-Regular", "monospace"],
      },
      fontSize: {
        // PR #55: weight + size step-up. The display + heading scale
        // now ships its own fontWeight so headings render bold
        // without callers having to remember `font-bold`. Body sizes
        // stay weightless — Tailwind's `font-medium` (PR #55 base on
        // <body>) carries them. Sizes themselves nudged up half a
        // step on display/h1/h2 to match the heavier weight.
        "display": [
          "2.5rem",
          { lineHeight: "2.75rem", letterSpacing: "-0.02em", fontWeight: "800" },
        ],
        "h1": [
          "1.875rem",
          { lineHeight: "2.25rem", letterSpacing: "-0.015em", fontWeight: "700" },
        ],
        "h2": [
          "1.5rem",
          { lineHeight: "2rem", letterSpacing: "-0.01em", fontWeight: "700" },
        ],
        "h3": [
          "1.125rem",
          { lineHeight: "1.5rem", fontWeight: "600" },
        ],
        "body": ["0.9375rem", { lineHeight: "1.5rem" }],
        "small": ["0.8125rem", { lineHeight: "1.25rem" }],
      },
      borderRadius: {
        card: "10px",
      },
    },
  },
  plugins: [],
};

export default config;
