import { expect, test, type Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";

/**
 * The Words tab on mocked data (`/voxa-preview?view=words`). Screenshots
 * (docs/screenshots) are only written with `bun run screenshots`.
 */

const SCREENSHOTS = process.env.VOXA_SCREENSHOTS === "1";
const OUT = path.join(process.cwd(), "docs", "screenshots");

const wordsUrl = (query = "", theme = "dark", lang = "ru") =>
  `/voxa-preview?view=words&theme=${theme}&lang=${lang}${query}`;

const row = (page: Page, testId: string, label: string) =>
  page.getByTestId(testId).locator("li", { hasText: label }).first();

test.describe("Words tab", () => {
  test("shows every block of the plan on the week", async ({ page }) => {
    await page.goto(wordsUrl());
    for (const id of [
      "words-vocabulary",
      "words-top",
      "words-openers",
      "words-phrases",
      "words-fillers",
      "words-new",
    ]) {
      await expect(page.getByTestId(id), id).toBeVisible();
    }
    const top = page.getByTestId("top-words").locator("li");
    await expect(top.first()).toContainText("промпт");
    await expect(top.first()).toContainText("41");
    // the vocabulary card
    await expect(page.getByTestId("words-vocabulary")).toContainText(
      "разных слов",
    );
  });

  test("function words are hidden until the switch is on, with no reload of the others", async ({
    page,
  }) => {
    await page.goto(wordsUrl());
    await expect(row(page, "top-words", "промпт")).toBeVisible();
    await expect(page.getByTestId("top-words")).not.toContainText("312");
    await page.getByLabel("Показывать служебные слова").check();
    // "и" is the most frequent word of the fixture
    const first = page.getByTestId("top-words").locator("li").first();
    await expect(first).toContainText("312");
    await page.getByLabel("Показывать служебные слова").uncheck();
    await expect(page.getByTestId("top-words")).not.toContainText("312");
  });

  test("the period filter drives every block", async ({ page }) => {
    await page.goto(wordsUrl());
    const first = () => page.getByTestId("top-words").locator("li").first();
    await expect(first()).toContainText("41");
    await page.getByRole("button", { name: "Месяц" }).click();
    await expect(first()).toContainText("164");
    await page.getByRole("button", { name: "Всё время" }).click();
    await expect(first()).toContainText("492");
    await expect(page.getByTestId("words-openers")).toContainText("21%");

    await page.getByRole("button", { name: "Свой период" }).click();
    await expect(page.getByLabel("С", { exact: true })).toBeVisible();
    await expect(page.getByLabel("По", { exact: true })).toBeVisible();
    await page.getByLabel("С", { exact: true }).fill("2026-10-01");
    await page.getByLabel("По", { exact: true }).fill("2026-10-07");
    await expect(first()).toContainText("41");
  });

  test("top words show 15 rows and expand to all", async ({ page }) => {
    await page.goto(wordsUrl());
    await page.getByLabel("Показывать служебные слова").check();
    const rows = page.getByTestId("top-words").locator("li");
    await expect(rows).toHaveCount(15);
    await page.getByRole("button", { name: /Показать все/ }).click();
    await expect(rows).toHaveCount(28);
    await page.getByRole("button", { name: "Свернуть" }).click();
    await expect(rows).toHaveCount(15);
  });

  test("openers, phrases, fillers and new words are listed", async ({
    page,
  }) => {
    await page.goto(wordsUrl());
    await expect(page.getByTestId("openers-one")).toContainText("короче");
    await expect(page.getByTestId("openers-two")).toContainText(
      "давай посмотрим",
    );
    await expect(page.getByTestId("phrases-three")).toContainText(
      "сделай так чтобы",
    );
    await expect(page.getByTestId("filler-list")).toContainText("как бы");
    await expect(page.getByTestId("new-words")).toContainText("колбэк");
  });

  test("the filler trend is a line with a readout that follows the point", async ({
    page,
  }) => {
    await page.goto(wordsUrl());
    const trend = page.getByTestId("filler-trend");
    await expect(trend.locator("path.vgw-trend-line")).toHaveCount(1);
    await expect(trend.locator("circle")).toHaveCount(12);
    const readout = trend.locator(".vgw-trend-readout");
    await expect(readout).toContainText("на 100 слов");
    const before = await readout.textContent();
    await trend.locator("circle").first().hover();
    await expect(readout).not.toHaveText(before ?? "");
    await expect(readout).toContainText("Неделя с");
  });

  test("a word opens the detail panel with its forms and highlighted examples", async ({
    page,
  }) => {
    await page.goto(wordsUrl());
    await row(page, "top-words", "модель").click();
    const panel = page.getByTestId("word-detail");
    await expect(panel).toBeVisible();
    await expect(panel.getByRole("heading", { level: 3 })).toHaveText("модель");
    await expect(panel).toContainText("Формы");
    await expect(panel.locator(".vgw-forms li")).toHaveCount(4);
    await expect(panel.locator(".vgw-examples li")).toHaveCount(5);
    await expect(panel.locator(".vgw-examples mark").first()).toHaveText(
      "модель",
    );
    // focus moves to the close button, Escape closes
    await expect(panel.getByRole("button", { name: "Закрыть" })).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(panel).toHaveCount(0);
  });

  test("a new word opens the panel too, and the scrim closes it", async ({
    page,
  }) => {
    await page.goto(wordsUrl());
    await page.getByTestId("new-words").getByRole("button").first().click();
    await expect(page.getByTestId("word-detail")).toBeVisible();
    await page.mouse.click(10, 10);
    await expect(page.getByTestId("word-detail")).toHaveCount(0);
  });

  test("no words at all shows the invitation", async ({ page }) => {
    await page.goto(wordsUrl("&words=empty"));
    await page.getByRole("button", { name: "Всё время" }).click();
    await expect(page.getByTestId("words-empty")).toContainText(
      "Пока нет слов",
    );
    await expect(page.getByTestId("words-top")).toHaveCount(0);
  });

  test("a failing block says so and offers a retry", async ({ page }) => {
    await page.goto(wordsUrl("&words=error"));
    const retries = page.getByRole("button", { name: "Повторить" });
    await expect(retries.first()).toBeVisible();
    await expect(page.getByRole("alert").first()).toContainText(
      "database is locked",
    );
  });

  test("the indexing of older dictations shows its progress", async ({
    page,
  }) => {
    await page.goto(wordsUrl("&words=backfill"));
    const banner = page.getByRole("status").filter({ hasText: "Индексирую" });
    await expect(banner).toContainText("1 200");
    await expect(banner).toContainText("5 000");
    await expect(banner.locator("progress")).toHaveAttribute("value", "1200");
  });

  test("works in English", async ({ page }) => {
    await page.goto(wordsUrl("", "light", "en"));
    await expect(page.getByTestId("words-top")).toContainText("Top words");
    await expect(page.getByTestId("words-fillers")).toContainText(
      "Filler words",
    );
    await expect(page.getByLabel("Show function words")).toBeVisible();
  });
});

test.describe("statistics page", () => {
  test("has two tabs and keeps the overview as the first", async ({ page }) => {
    await page.goto("/voxa-preview?view=app&theme=dark&lang=ru");
    await page.getByText("Статистика", { exact: true }).first().click();
    const overview = page.getByRole("tab", { name: "Обзор" });
    const words = page.getByRole("tab", { name: "Слова" });
    await expect(overview).toHaveAttribute("aria-selected", "true");
    await expect(page.getByText("Слова по дням")).toBeVisible();
    await words.click();
    await expect(words).toHaveAttribute("aria-selected", "true");
    await expect(page.getByTestId("words-top")).toBeVisible();
    await overview.click();
    await expect(page.getByText("Слова по дням")).toBeVisible();
  });
});

test.describe("filler word settings", () => {
  test("the list is editable in Advanced settings", async ({ page }) => {
    await page.goto("/voxa-preview?view=app&theme=dark&lang=ru");
    await page.getByText("Продвинутые", { exact: true }).first().click();
    const input = page.getByTestId("filler-words-input");
    await expect(input).toHaveValue(/короче, ну, как бы/);
  });
});

test.describe("word screenshots", () => {
  test.skip(
    !SCREENSHOTS,
    "set VOXA_SCREENSHOTS=1 (bun run screenshots) to write docs/screenshots",
  );

  for (const theme of ["dark", "light"] as const) {
    test(`words tab, ${theme}`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: theme });
      await page.setViewportSize({ width: 740, height: 2300 });
      await page.goto(wordsUrl("", theme));
      await page.waitForTimeout(600);
      fs.mkdirSync(OUT, { recursive: true });
      await page.getByTestId("words-view").screenshot({
        path: path.join(OUT, `words-${theme}.png`),
      });
    });
  }

  test("words tab in the app window, with the detail panel", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1100, height: 760 });
    await page.goto("/voxa-preview?view=app&theme=dark&lang=ru");
    await page.getByText("Статистика", { exact: true }).first().click();
    await page.getByRole("tab", { name: "Слова" }).click();
    await page.waitForTimeout(500);
    fs.mkdirSync(OUT, { recursive: true });
    await page.screenshot({ path: path.join(OUT, "words-app-dark.png") });
    await row(page, "top-words", "модель").click();
    await page.waitForTimeout(500);
    await page.screenshot({
      path: path.join(OUT, "words-detail-dark.png"),
    });
  });

  test("words tab while indexing", async ({ page }) => {
    await page.setViewportSize({ width: 740, height: 700 });
    await page.goto(wordsUrl("&words=backfill"));
    await page.waitForTimeout(500);
    fs.mkdirSync(OUT, { recursive: true });
    await page.screenshot({ path: path.join(OUT, "words-indexing-dark.png") });
  });
});
