import { test, expect } from "@playwright/test";
import { setupMocks } from "./helpers";

test.describe("Output Language Settings", () => {
  test.beforeEach(async ({ page }) => {
    await setupMocks(page);
  });

  test("allows selecting different output languages", async ({ page }) => {
    // Go to home page (which defaults to General Settings)
    await page.goto("/");

    // Verify Output Language setting group exists by checking for its title
    const titleLocator = page.locator("text=Output Language");
    await expect(titleLocator).toBeVisible();

    // Verify option buttons are visible
    const malayalamBtn = page.locator("button:has-text('Malayalam')");
    const manglishBtn = page.locator("button:has-text('Manglish')");
    const englishBtn = page.locator("button:has-text('English')");

    await expect(malayalamBtn).toBeVisible();
    await expect(manglishBtn).toBeVisible();
    await expect(englishBtn).toBeVisible();

    // By default, Malayalam should be selected
    await expect(malayalamBtn).toHaveClass(/bg-logo-primary/);
    await expect(manglishBtn).not.toHaveClass(/bg-logo-primary/);

    // Select Manglish
    await manglishBtn.click();

    // Verify styling shifts to selected state for Manglish
    await expect(manglishBtn).toHaveClass(/bg-logo-primary/);
    await expect(malayalamBtn).not.toHaveClass(/bg-logo-primary/);

    // Refresh page to verify persistence
    await page.reload();

    // Verify Manglish is still selected after reload
    await expect(page.locator("button:has-text('Manglish')")).toHaveClass(
      /bg-logo-primary/,
    );
    await expect(page.locator("button:has-text('Malayalam')")).not.toHaveClass(
      /bg-logo-primary/,
    );
  });
});
