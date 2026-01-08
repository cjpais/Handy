/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  theme: {
    extend: {
      colors: {
        text: "var(--color-text)",
        background: "var(--color-background)",
        "logo-primary": "var(--color-logo-primary)",
        "logo-secondary": "var(--color-logo-secondary)",
        "logo-stroke": "var(--color-logo-stroke)",
        "text-stroke": "var(--color-text-stroke)",
        "accent-cyan": "var(--color-accent-cyan)",
        "accent-purple": "var(--color-accent-purple)",
      },
    },
  },
  plugins: [],
};
