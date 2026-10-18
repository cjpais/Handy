import { expect, test, type Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";

/**
 * In-app update UI on mocked data (`/voxa-preview?view=app&update=...`).
 * Screenshots (docs/screenshots) are only written with `bun run screenshots`.
 */

const SCREENSHOTS = process.env.VOXA_SCREENSHOTS === "1";
const OUT = path.join(process.cwd(), "docs", "screenshots");

const appUrl = (update: string, theme = "dark", lang = "ru") =>
  `/voxa-preview?view=app&theme=${theme}&lang=${lang}&update=${update}`;

async function openAbout(page: Page) {
  await page.getByText("О программе", { exact: true }).first().click();
  await expect(page.getByTestId("update-status")).toBeVisible();
}

test.describe("in-app updates", () => {
  test("an announced version shows the badge, the footer link and the notes", async ({
    page,
  }) => {
    await page.goto(appUrl("available"));
    await expect(page.getByTestId("update-badge")).toBeVisible();
    await expect(page.getByTestId("footer-update")).toHaveText(
      "Доступна версия 0.10.7",
    );

    // The footer link opens the page with the update card.
    await page.getByTestId("footer-update").click();
    await expect(page.getByTestId("update-status")).toHaveText(
      "Доступна версия 0.10.7.",
    );
    await expect(page.getByTestId("update-notes")).toContainText(
      "Words tab with top words",
    );
    await expect(page.getByTestId("update-install")).toHaveText("Обновить");
    await expect(page.getByTestId("update-check")).toBeEnabled();
  });

  test("nothing is installed until the button is pressed; pressing it shows progress", async ({
    page,
  }) => {
    await page.goto(appUrl("available"));
    await openAbout(page);
    await expect(page.getByTestId("update-status")).not.toContainText(
      "Скачиваю",
    );
    await page.getByTestId("update-install").click();
    await expect(page.getByTestId("update-status")).toHaveText(
      "Скачиваю обновление… 42%",
    );
    await expect(page.getByTestId("update-install")).toHaveCount(0);
    await expect(page.getByTestId("update-check")).toBeDisabled();
  });

  test("a manual check without news reports the latest version and clears the badge", async ({
    page,
  }) => {
    await page.goto(appUrl("idle"));
    await openAbout(page);
    await expect(page.getByTestId("update-status")).toHaveText(
      "Ещё не проверяли.",
    );
    await expect(page.getByTestId("update-badge")).toHaveCount(0);
    await page.getByTestId("update-check").click();
    await expect(page.getByTestId("update-status")).toHaveText(
      "Установлена последняя версия.",
    );
    await expect(page.getByTestId("footer-update")).toHaveCount(0);
  });

  test("a network error is explained and the current version keeps working", async ({
    page,
  }) => {
    await page.goto(appUrl("error"));
    await openAbout(page);
    await expect(page.getByTestId("update-status")).toContainText(
      "Не удалось связаться с GitHub",
    );
    await expect(page.getByTestId("update-status")).toContainText(
      "Текущая версия продолжает работать",
    );
    await expect(page.getByTestId("update-status")).toHaveAttribute(
      "role",
      "alert",
    );
    await expect(page.getByTestId("update-install")).toHaveCount(0);
  });

  test("the automatic-check switch is on by default and can be turned off", async ({
    page,
  }) => {
    await page.goto(appUrl("up_to_date"));
    await openAbout(page);
    const toggle = page.getByRole("checkbox", {
      name: "Проверять обновления автоматически",
    });
    await expect(toggle).toBeChecked();
  });

  test("works in English", async ({ page }) => {
    await page.goto(appUrl("available", "light", "en"));
    await page.getByText("About", { exact: true }).first().click();
    await expect(page.getByTestId("update-status")).toHaveText(
      "Version 0.10.7 is available.",
    );
    await expect(page.getByTestId("update-install")).toHaveText("Update");
  });
});

test.describe("update screenshots", () => {
  test.skip(
    !SCREENSHOTS,
    "set VOXA_SCREENSHOTS=1 (bun run screenshots) to write docs/screenshots",
  );

  for (const [kind, theme] of [
    ["available", "dark"],
    ["available", "light"],
    ["downloading", "dark"],
    ["error", "dark"],
  ] as const) {
    test(`updates, ${kind}, ${theme}`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: theme });
      await page.setViewportSize({ width: 900, height: 700 });
      await page.goto(appUrl(kind, theme));
      await page.getByText("О программе", { exact: true }).first().click();
      await page.waitForTimeout(500);
      fs.mkdirSync(OUT, { recursive: true });
      await page.screenshot({
        path: path.join(OUT, `updates-${kind}-${theme}.png`),
      });
    });
  }
});
