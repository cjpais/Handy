import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { resolve } from "path";

const host = process.env.TAURI_DEV_HOST;

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  plugins: [react(), tailwindcss()],

  // Path aliases
  resolve: {
    alias: {
      "@/": resolve(__dirname, "./src/"),
      "@/bindings": resolve(__dirname, "./src/bindings.ts"),
      "@/i18n": resolve(__dirname, "./src/i18n"),
      "@/stores": resolve(__dirname, "./src/stores"),
      "@/components": resolve(__dirname, "./src/components"),
      "@/hooks": resolve(__dirname, "./src/hooks"),
    },
  },

  // Multiple entry points for main app and overlay
  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        overlay: resolve(__dirname, "src/overlay/index.html"),
      },
    },
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
