/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  theme: {
    extend: {
      colors: {
        surface: {
          base: "rgb(var(--bg-base-rgb) / <alpha-value>)",
          panel: "rgb(var(--bg-panel-rgb) / <alpha-value>)",
          border: "rgb(var(--bg-border-rgb) / <alpha-value>)",
          muted: "rgb(var(--text-muted-rgb) / <alpha-value>)",
          text: "rgb(var(--text-primary-rgb) / <alpha-value>)",
        },
        accent: {
          blue: "rgb(var(--accent-blue-rgb) / <alpha-value>)",
          purple: "rgb(var(--accent-purple-rgb) / <alpha-value>)",
        },
        diff: {
          add: "rgb(var(--diff-add-rgb) / <alpha-value>)",
          remove: "rgb(var(--diff-remove-rgb) / <alpha-value>)",
          modify: "rgb(var(--diff-modify-rgb) / <alpha-value>)",
        },
      },
      fontFamily: {
        mono: ["JetBrains Mono", "Fira Code", "Consolas", "monospace"],
        sans: [
          "-apple-system",
          "BlinkMacSystemFont",
          "Segoe UI",
          "Noto Sans",
          "Helvetica",
          "Arial",
          "sans-serif",
        ],
      },
      animation: {
        "pulse-dot": "pulse-dot 1.4s ease-in-out infinite",
        "fade-in": "fade-in 0.2s ease-out",
        "slide-up": "slide-up 0.2s ease-out",
      },
      keyframes: {
        "pulse-dot": {
          "0%, 100%": { opacity: "0.3" },
          "50%": { opacity: "1" },
        },
        "fade-in": {
          from: { opacity: "0" },
          to: { opacity: "1" },
        },
        "slide-up": {
          from: { opacity: "0", transform: "translateY(4px)" },
          to: { opacity: "1", transform: "translateY(0)" },
        },
      },
    },
  },
  plugins: [],
};
