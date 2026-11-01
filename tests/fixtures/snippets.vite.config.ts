import { mergeConfig } from "vite";
import baseConfig from "../../vite.config";

// Include the dynamically imported fixture in dependency discovery so the
// first test does not reload halfway through editing a draft.
export default mergeConfig(
  await baseConfig({ command: "serve", mode: "test" }),
  {
    cacheDir: "test-results/.vite-snippets",
    optimizeDeps: { entries: ["tests/fixtures/snippets.tsx"] },
  },
);
