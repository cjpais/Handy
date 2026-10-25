import { test, expect } from "@playwright/test";
import type { AvailableAccelerators } from "../src/bindings";

type TestWindow = Window & {
  accelerationTest: {
    available: AvailableAccelerators;
    writes: unknown[];
    listeners: Record<string, () => void>;
  };
};

// Use the installed browser on Windows; elsewhere use Playwright's Chromium.
test.use(process.platform === "win32" ? { channel: "msedge" } : {});

test("fallback status is amber, preserves preference, and disappears on recovery", async ({
  page,
}) => {
  await page.addInitScript(() => {
    Object.assign(window, {
      accelerationTest: {
        settings: {
          transcribe_accelerator: "gpu",
          transcribe_gpu_device: "preferred_gpu",
        },
        writes: [],
        listeners: {},
        available: {
          transcribe: ["auto", "gpu", "cpu"],
          gpu_devices: [
            {
              id: "preferred_gpu",
              name: "Preferred GPU",
              total_vram_mb: 6144,
              available: true,
            },
            {
              id: "alternate_gpu",
              name: "Alternate GPU",
              total_vram_mb: 2048,
              available: true,
            },
          ],
          active_device: {
            name: "Preferred GPU",
            on_gpu: true,
            is_fallback: false,
          },
        },
      },
    });
  });
  // Mock only the native boundary; render the real setting and translations.
  await page.route("**/src/hooks/useSettings.ts*", (route) =>
    route.fulfill({
      contentType: "application/javascript",
      body: `export function useSettings() { const s = window.accelerationTest; return {
      getSetting: key => s.settings[key], isUpdating: () => false,
      updateSetting: async (key, value) => { s.writes.push([key, value]); s.settings[key] = value; }
    }; }`,
    }),
  );
  await page.route("**/src/bindings.ts*", (route) =>
    route.fulfill({
      contentType: "application/javascript",
      body: "export const commands = { getAvailableAccelerators: async () => structuredClone(window.accelerationTest.available) };",
    }),
  );
  await page.route("**/@tauri-apps_api_event.js*", (route) =>
    route.fulfill({
      contentType: "application/javascript",
      body: `export async function listen(name, cb) {
      window.accelerationTest.listeners[name] = cb;
      return () => delete window.accelerationTest.listeners[name];
    }`,
    }),
  );
  await page.goto("/tests/fixtures/acceleration.html");
  const selected = page.getByRole("button", { name: /Preferred GPU/ });
  await expect(selected).toBeVisible();
  const status = page.locator('p[aria-live="polite"]');
  await expect(status).toHaveCount(0);

  const updateDevice = async (
    name: string,
    on_gpu: boolean,
    is_fallback: boolean,
  ) => {
    await page.evaluate(
      ({ name, on_gpu, is_fallback }) => {
        const state = (window as TestWindow).accelerationTest;
        state.available.active_device = {
          name,
          on_gpu,
          is_fallback,
          is_pending: false,
        };
        state.listeners["transcription-device-changed"]();
      },
      { name, on_gpu, is_fallback },
    );
  };
  await updateDevice("Alternate GPU", true, true);
  await expect(status).toContainText("Alternate GPU (fallback)");
  await expect(status).toHaveClass(/text-amber-600/);
  await expect(selected).toBeVisible();

  await updateDevice("CPU", false, true);
  await expect(status).toContainText("your computer's CPU (fallback)");
  await expect(selected).toBeVisible();

  // A retry can bind the alternate GPU, then fail again before successful inference. Status
  // must follow both bindings even before successful inference.
  await updateDevice("Alternate GPU", true, true);
  await expect(status).toContainText("Alternate GPU (fallback)");
  await updateDevice("CPU", false, true);
  await expect(status).toContainText("your computer's CPU (fallback)");

  await updateDevice("Preferred GPU", true, false);
  await expect(status).toHaveCount(0);
  await expect(selected).toBeVisible();
  const writes = await page.evaluate(
    () => (window as TestWindow).accelerationTest.writes,
  );
  expect(writes).toEqual([]);
});
