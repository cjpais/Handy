import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { resolve } from "path";

export default defineConfig({
  plugins: [react(), tailwindcss()],

  root: resolve(__dirname, "./storybook"),

  resolve: {
    alias: {
      "@": resolve(__dirname, "./src"),
      "@/bindings": resolve(__dirname, "./src/bindings.ts"),
    },
    // Look for node_modules in Handy-main, not in the storybook folder
    modules: [resolve(__dirname, "node_modules"), "node_modules"],
  },

  server: {
    port: 1422,
    strictPort: true,
    fs: {
      allow: [__dirname],
    },
  },
});
