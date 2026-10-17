import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { resolve } from "path";
import type { Plugin } from "vite";

const host = process.env.TAURI_DEV_HOST;

// Voxa: `/voxa-preview` is a dev-only page (overlay gallery, mocked app) used for
// screenshots. It is not a build input, so it never ships in the app bundle.
const voxaPreviewRoute = (): Plugin => ({
  name: "voxa-preview-route",
  apply: "serve",
  configureServer(server) {
    server.middlewares.use((req, _res, next) => {
      const [path, query = ""] = (req.url ?? "").split("?");
      if (path === "/voxa-preview" || path === "/voxa-preview/") {
        req.url = `/voxa-preview/index.html${query ? `?${query}` : ""}`;
      }
      next();
    });
  },
});

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  plugins: [react(), tailwindcss(), voxaPreviewRoute()],

  // Path aliases
  resolve: {
    alias: {
      "@": resolve(__dirname, "./src"),
      "@/bindings": resolve(__dirname, "./src/bindings.ts"),
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
