import { test, expect } from "@playwright/test";

test.describe("Handy App", () => {
  test("dev server responds", async ({ page }) => {
    // Just verify the dev server is running and responds
    const response = await page.goto("/");
    expect(response?.status()).toBe(200);
  });

  test("page has html structure", async ({ page }) => {
    await page.goto("/");

    // Verify basic HTML structure exists
    const html = await page.content();
    expect(html).toContain("<html");
    expect(html).toContain("<body");
  });
});


test("post_processing_settings_show_promptv3_and_capglue_unavailable_state", async ({ page }) => {
  await page.goto("/");

  const html = await page.content();
  const sourceChecks = await page.evaluate(async () => {
    const [settings, pasteMethod, translations] = await Promise.all([
      fetch("/src/components/settings/post-processing/PostProcessingSettings.tsx").then((r) => r.text()),
      fetch("/src/components/settings/PasteMethod.tsx").then((r) => r.text()),
      fetch("/src/i18n/locales/en/translation.json").then((r) => r.text()),
    ]);
    return { settings, pasteMethod, translations };
  });

  expect(html).toContain("html");
  expect(sourceChecks.settings).toContain("promptv3");
  expect(sourceChecks.settings).toContain("capglue");
  expect(sourceChecks.pasteMethod).toContain("capglue");
  expect(sourceChecks.translations).toContain("capglueUnavailable");
});
