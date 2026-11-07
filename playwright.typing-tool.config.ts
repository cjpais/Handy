import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./tests",
  testMatch: "typing-tool.spec.ts",
  reporter: "list",
  use: {
    ...devices["Desktop Chrome"],
    channel: process.env.PLAYWRIGHT_CHANNEL,
    baseURL: "http://127.0.0.1:1435",
  },
  webServer: {
    command: "bun x vite --host 127.0.0.1 --port 1435 --strictPort",
    url: "http://127.0.0.1:1435",
    reuseExistingServer: false,
  },
});
