import { defineConfig } from "@playwright/test";
import baseConfig from "../playwright.config";
import { fileURLToPath } from "node:url";

export default defineConfig({
  ...baseConfig,
  testDir: ".",
  testMatch: "snippets.spec.ts",
  use: { ...baseConfig.use, baseURL: "http://localhost:1432" },
  projects: baseConfig.projects?.map((project) => ({
    ...project,
    use: {
      ...project.use,
      channel: process.env.PLAYWRIGHT_CHROMIUM_CHANNEL,
    },
  })),
  webServer: {
    cwd: fileURLToPath(new URL("..", import.meta.url)),
    command:
      "node node_modules/vite/bin/vite.js --config tests/fixtures/snippets.vite.config.ts --port 1432",
    url: "http://localhost:1432",
    reuseExistingServer: false,
  },
});
