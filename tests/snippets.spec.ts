import { test, expect, type Page } from "@playwright/test";

// Component UI fixture only: this exercises the actual Snippets component and
// translations with an in-memory settings writer, not Tauri persistence or audio.
const fixture = `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>Snippets component UI test</title></head>
<body><main id="root"></main><script type="module">
import RefreshRuntime from '/@react-refresh';
RefreshRuntime.injectIntoGlobalHook(window);
window.$RefreshReg$ = () => {};
window.$RefreshSig$ = () => (type) => type;
window.__vite_plugin_react_preamble_installed__ = true;
await import('/tests/fixtures/snippets.tsx');
</script></body></html>`;

const openFixture = async (page: Page) => {
  await page.route("**/__snippets-test", (route) =>
    route.fulfill({ contentType: "text/html", body: fixture }),
  );
  await page.goto("/__snippets-test");
  await expect(
    page.getByRole("heading", { name: "Add a snippet" }),
  ).toBeVisible();
};

const fillDraft = async (page: Page, phrase: string, replacement: string) => {
  await page.getByLabel("Spoken phrase (up to 100 characters)").fill(phrase);
  await page
    .getByLabel("Replacement text (up to 4,000 characters)")
    .fill(replacement);
};

test.describe("Snippets component UI (in-memory persistence fixture)", () => {
  test.beforeEach(async ({ page }) => openFixture(page));

  test("adds, edits, toggles, and removes a multiline snippet", async ({
    page,
  }, testInfo) => {
    const globalSwitch = page.getByRole("checkbox", {
      name: "Snippets",
      exact: true,
    });
    await expect(globalSwitch).not.toBeChecked();
    await globalSwitch.locator("..").click();
    await expect(globalSwitch).toBeChecked();

    await fillDraft(page, "my signature", "Alan\nBest wishes");
    await page
      .getByRole("button", { name: "Add snippet", exact: true })
      .click();
    const row = page.getByRole("listitem");
    await expect(row).toHaveCount(1);
    await expect(row.locator("p")).toHaveText("Alan\nBest wishes");
    await page.screenshot({
      path: testInfo.outputPath("snippets-settings.png"),
      fullPage: true,
    });
    await expect(
      page.getByLabel("Spoken phrase (up to 100 characters)"),
    ).toHaveValue("");

    const rowSwitch = page.getByRole("checkbox", {
      name: "Enable snippet: my signature",
    });
    await expect(rowSwitch).toBeChecked();
    await rowSwitch.uncheck();
    await expect(rowSwitch).not.toBeChecked();

    await page
      .getByRole("button", { name: "Edit snippet: my signature" })
      .click();
    await fillDraft(page, "sign off", "Alan\nThank you");
    await page.getByRole("button", { name: "Save changes" }).click();
    await expect(row.locator("p")).toHaveText("Alan\nThank you");
    await expect(
      page.getByRole("checkbox", { name: "Enable snippet: sign off" }),
    ).not.toBeChecked();

    await page
      .getByRole("button", { name: "Remove snippet: sign off" })
      .click();
    await expect(row).toHaveCount(0);
    await globalSwitch.locator("..").click();
    await expect(globalSwitch).not.toBeChecked();
  });

  test("rejects phrases duplicated by letter case and extra spaces", async ({
    page,
  }) => {
    await fillDraft(page, "my email", "alan@example.com");
    await page
      .getByRole("button", { name: "Add snippet", exact: true })
      .click();
    await fillDraft(page, "  MY   EMAIL  ", "other@example.com");
    await page
      .getByRole("button", { name: "Add snippet", exact: true })
      .click();
    await expect(page.getByRole("alert")).toContainText(
      "A snippet already uses this phrase",
    );
    await expect(page.getByRole("listitem")).toHaveCount(1);
    await expect(
      page.getByLabel("Spoken phrase (up to 100 characters)"),
    ).toHaveValue("  MY   EMAIL  ");
    await expect(
      page.getByLabel("Replacement text (up to 4,000 characters)"),
    ).toHaveValue("other@example.com");
  });

  test("retains an unsaved draft after a write fails and supports retry", async ({
    page,
  }) => {
    await fillDraft(page, "my address", "First line\nSecond line");
    await page.evaluate(() => {
      const fixtureWindow = window as unknown as {
        snippetsFixture: { failNextSave: boolean };
      };
      fixtureWindow.snippetsFixture.failNextSave = true;
    });
    await page
      .getByRole("button", { name: "Add snippet", exact: true })
      .click();
    await expect(page.getByRole("alert")).toContainText(
      "Could not save snippets",
    );
    await expect(page.getByRole("listitem")).toHaveCount(0);
    await expect(
      page.getByLabel("Spoken phrase (up to 100 characters)"),
    ).toHaveValue("my address");
    await expect(
      page.getByLabel("Replacement text (up to 4,000 characters)"),
    ).toHaveValue("First line\nSecond line");

    await page
      .getByRole("button", { name: "Add snippet", exact: true })
      .click();
    await expect(page.getByRole("listitem")).toHaveCount(1);
    await expect(page.getByRole("alert")).toHaveCount(0);
    await expect(
      page.getByLabel("Spoken phrase (up to 100 characters)"),
    ).toHaveValue("");
  });
});
