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
          bg: "#0f1117",
          surface: "#1a1c23",
          "surface-2": "#21242e",
          border: "#30363d",
          "border-light": "#3d4450",
          text: "#c8ccd4",
          "text-bright": "#ffffff",
          "text-dim": "#6b7280",
          red: "#e94560",
          "red-dim": "rgba(233, 69, 96, 0.15)",
          purple: "#6a5acd",
          "purple-mid": "rgba(106, 90, 205, 0.25)",
          green: "#22c55e",
          yellow: "#f59e0b",
          "glass-trans": "rgba(31, 41, 55, 0.2)",
          "glass-opaque": "#21242e",
          "modal-overlay": "rgba(15, 17, 23, 0.8)",
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
