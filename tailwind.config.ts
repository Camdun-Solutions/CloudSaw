import type { Config } from "tailwindcss";

const config: Config = {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  // PR #57: class-based dark mode. The <html> element gets a `dark`
  // class set by useAppearance() based on the user's Settings →
  // Appearance choice (light / dark / system).
  darkMode: "class",
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
          // PR #57: dark-mode palette. Per user spec: dark grey,
          // beige, red, black.
          //   - `grey-dark` is the dark-mode elevated-surface
          //     background (cards, modals, nav bars) — sits one
          //     step lighter than `black` so depth reads.
          //   - `beige` is the dark-mode body text color — a warm
          //     off-white that's gentler on the eyes than pure
          //     white in a dark room.
          //   - `black` (existing) is the dark-mode page background.
          //   - `red` / `red-bold` (existing) carry through both
          //     modes — accent doesn't shift.
          "grey-dark": "#1A1B1F",
          beige: "#E8DCC4",
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
        "display": ["2.25rem", { lineHeight: "2.5rem", letterSpacing: "-0.02em" }],
        "h1": ["1.75rem", { lineHeight: "2rem", letterSpacing: "-0.01em" }],
        "h2": ["1.375rem", { lineHeight: "1.75rem" }],
        "h3": ["1.125rem", { lineHeight: "1.5rem" }],
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
