/** @type {import('tailwindcss').Config} */
module.exports = {
  darkMode: ["class"],
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      borderColor: {
        border: 'hsl(var(--border))',
      },
      colors: {
        lagado: {
          // Backgrounds — deep navy with slight blue tint (Liquid AI inspired)
          bg:           "#080c14",
          surface:      "#0d1220",
          "surface-2":  "#131928",
          border:       "#1e2d47",
          "border-light":"#263852",

          // Text
          text:         "#c4cfe8",
          "text-bright":"#e8eeff",
          "text-dim":   "#4d6080",

          // Primary accent — electric blue (interactive elements, focus rings, CTA)
          blue:         "#3b82f6",
          "blue-dim":   "rgba(59, 130, 246, 0.12)",
          "blue-glow":  "rgba(59, 130, 246, 0.25)",

          // Secondary accent — purple (hover states, secondary actions, Liquid brand)
          purple:       "#8b5cf6",
          "purple-dim": "rgba(139, 92, 246, 0.12)",
          "purple-glow":"rgba(139, 92, 246, 0.25)",

          // Status — keep semantic meaning
          red:          "#ef4444",   // destructive / deny / error only
          "red-dim":    "rgba(239, 68, 68, 0.12)",
          green:        "#22c55e",   // connected / approved / success only
          yellow:       "#f59e0b",   // warning / connecting

          // Utility
          "glass-trans":   "rgba(13, 18, 32, 0.6)",
          "glass-opaque":  "#131928",
          "modal-overlay": "rgba(8, 12, 20, 0.85)",
        },
      },
      fontFamily: {
        rajdhani: ["Rajdhani", "sans-serif"],
        mono: ["Share Tech Mono", "monospace"],
      },
      fontSize: {
        h1: ["24px", { lineHeight: "32px", fontWeight: "700" }],
        h2: ["20px", { lineHeight: "28px", fontWeight: "600" }],
        h3: ["16px", { lineHeight: "22px", fontWeight: "600" }],
        body: ["15px", { lineHeight: "22px", fontWeight: "400" }],
        "body-sm": ["13px", { lineHeight: "20px", fontWeight: "400" }],
        btn: ["13px", { lineHeight: "18px", fontWeight: "500" }],
        caption: ["11px", { lineHeight: "16px", fontWeight: "400" }],
      },
    },
  },
  plugins: [],
};
