import { expect, test, type Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";

/**
 * Voxa's visual checks run against `/voxa-preview`, where the Tauri backend is
 * replaced by fixtures. The functional checks always run; the screenshots
 * (docs/screenshots) are only written with `bun run screenshots`.
 */

const SCREENSHOTS = process.env.VOXA_SCREENSHOTS === "1";
const OUT = path.join(process.cwd(), "docs", "screenshots");

const STAGES = [
  ["dark-arming", "arming"],
  ["dark-recording", "recording"],
  ["dark-transcribing", "transcribing"],
  ["dark-done", "done"],
  ["dark-error", "error"],
  ["light-arming", "arming"],
  ["light-recording", "recording"],
  ["light-transcribing", "transcribing"],
  ["light-done", "done"],
  ["light-error", "error"],
  ["busy-dark-recording", "recording"],
  ["busy-dark-error", "error"],
  ["busy-light-recording", "recording"],
  ["busy-light-error", "error"],
] as const;

const overlayUrl = (query = "") => `/voxa-preview?view=overlay&lang=ru${query}`;

async function shot(locator: ReturnType<Page["locator"]>, name: string) {
  fs.mkdirSync(OUT, { recursive: true });
  await locator.screenshot({ path: path.join(OUT, `${name}.png`) });
}

test.describe("overlay capsule", () => {
  test("renders every state on every backdrop", async ({ page }) => {
    await page.goto(overlayUrl());
    for (const [id, state] of STAGES) {
      const pill = page.locator(`[data-testid="stage-${id}"] .vg-pill`);
      await expect(pill, id).toHaveClass(new RegExp(`is-${state}`));
      await expect(pill, id).toHaveCSS("width", "220px");
      await expect(pill, id).toHaveCSS("height", "48px");
      await expect(pill, id).toHaveCSS("border-top-left-radius", "24px");
    }
  });

  test("recording shows the timer and seven bars; done shows a check mark", async ({
    page,
  }) => {
    await page.goto(overlayUrl());
    const recording = page.locator(
      '[data-testid="stage-dark-recording"] .vg-pill',
    );
    await expect(recording.locator(".vg-timer")).toHaveText("0:07");
    await expect(recording.locator(".vg-wave i")).toHaveCount(7);
    const done = page.locator('[data-testid="stage-dark-done"] .vg-pill');
    await expect(done.locator(".vg-check")).toBeVisible();
    await expect(done).toHaveAttribute("aria-label", "Готово");
    const error = page.locator('[data-testid="stage-dark-error"] .vg-pill');
    await expect(error.locator(".vg-error-text")).toHaveText(
      "Не удалось вставить",
    );
  });

  test("the cancel button appears on hover and is reachable by keyboard", async ({
    page,
  }) => {
    await page.goto(overlayUrl("&only=dark-recording"));
    const pill = page.locator(".vg-pill");
    const cancel = pill.locator(".vg-x");
    await expect(cancel).toHaveCSS("opacity", "0");
    await pill.hover();
    await expect(cancel).toHaveCSS("opacity", "1");
    await page.mouse.move(0, 0);
    await expect(cancel).toHaveCSS("opacity", "0");
    await cancel.focus();
    await expect(cancel).toHaveCSS("opacity", "1");
    await expect(cancel).toHaveAccessibleName("Отменить диктовку");
  });

  test("reduced motion removes the shimmer, the pulse and the spring", async ({
    page,
  }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto(overlayUrl());
    const flow = page.locator(
      '[data-testid="stage-dark-transcribing"] .vg-flow',
    );
    await expect(flow).toHaveCSS("animation-name", "none");
    const arming = page
      .locator('[data-testid="stage-dark-arming"] .vg-wave i')
      .first();
    await expect(arming).toHaveCSS("animation-name", "none");
    const pill = page.locator('[data-testid="stage-dark-recording"] .vg-pill');
    await expect(pill).toHaveCSS("animation-name", "vg-fade");
  });

  test("motion is on by default", async ({ page }) => {
    await page.goto(overlayUrl());
    const flow = page.locator(
      '[data-testid="stage-dark-transcribing"] .vg-flow',
    );
    await expect(flow).toHaveCSS("animation-name", "vg-shimmer");
    const pill = page.locator('[data-testid="stage-dark-recording"] .vg-pill');
    await expect(pill).toHaveCSS("animation-name", "vg-pop");
  });

  test("reduced transparency swaps the glass for an opaque capsule", async ({
    page,
  }) => {
    const client = await page.context().newCDPSession(page);
    await client.send("Emulation.setEmulatedMedia", {
      features: [{ name: "prefers-reduced-transparency", value: "reduce" }],
    });
    await page.goto(overlayUrl());
    const pill = page.locator('[data-testid="stage-dark-recording"] .vg-pill');
    await expect(pill).toHaveCSS("backdrop-filter", "none");
    const background = await pill.evaluate(
      (el) => getComputedStyle(el).backgroundColor,
    );
    expect(background).toMatch(/rgb\(|rgba\([^)]*,\s*1\)/);
    expect(background).not.toMatch(/0\.\d+\)$/);
  });

  test("the glass material is applied (blur, saturation, gradient rim)", async ({
    page,
  }) => {
    await page.goto(overlayUrl());
    const pill = page.locator('[data-testid="stage-dark-recording"] .vg-pill');
    const backdrop = await pill.evaluate(
      (el) => getComputedStyle(el).backdropFilter,
    );
    expect(backdrop).toContain("blur(24px)");
    expect(backdrop).toContain("saturate(1.8)");
    const rim = await pill.evaluate(
      (el) => getComputedStyle(el, "::before").backgroundImage,
    );
    expect(rim).toContain("linear-gradient");
  });
});

test.describe("statistics screen", () => {
  test("shows the numbers, the not-enough-data note and the candidates", async ({
    page,
  }) => {
    await page.goto("/voxa-preview?view=stats&theme=dark&lang=ru");
    const text = (await page.locator(".vg-stats").innerText()).replace(
      /\s/g,
      " ",
    );
    for (const expected of [
      "412",
      "2 134",
      "6 420",
      "27%",
      "Максимум: 522",
      "мало данных",
    ]) {
      expect(text).toContain(expected);
    }
    await expect(
      page.getByRole("button", { name: "Добавить в словарь" }),
    ).toHaveCount(3);
    await expect(page.getByText("Уже в словаре")).toHaveCount(3);
    await expect(page.locator(".vg-bar-slot")).toHaveCount(30);
    await expect(page.locator(".vg-bar.is-today")).toHaveCount(1);
  });

  test("the empty state invites the first dictation", async ({ page }) => {
    await page.goto("/voxa-preview?view=app&theme=light&lang=ru&data=empty");
    await page.getByText("Статистика", { exact: true }).first().click();
    await expect(page.getByText("Пока нет диктовок")).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Очистить статистику" }),
    ).toHaveCount(0);
  });

  test("works in English", async ({ page }) => {
    await page.goto("/voxa-preview?view=stats&theme=light&lang=en");
    await expect(page.getByText("Words per day")).toBeVisible();
    await expect(page.getByText("not enough data")).toHaveCount(2);
  });
});

test.describe("app on mocked data", () => {
  test("statistics section opens from the sidebar and the add button works", async ({
    page,
  }) => {
    await page.goto("/voxa-preview?view=app&theme=light&lang=ru");
    await page.getByText("Статистика", { exact: true }).first().click();
    await expect(page.getByText("Слова по дням")).toBeVisible();
    await page
      .getByRole("button", { name: "Добавить в словарь" })
      .first()
      .click();
    await expect(page.getByText(/добавлено в словарь/)).toBeVisible();
  });

  test("app shows the Voxa name and no Handy branding in the chrome", async ({
    page,
  }) => {
    await page.goto("/voxa-preview?view=app&theme=dark&lang=en");
    await expect(page.getByRole("img", { name: "Voxa" })).toBeVisible();
    const body = await page.locator("body").innerText();
    expect(body).not.toMatch(/Support Development|Donate/);
  });
});

test.describe("screenshots", () => {
  test.skip(
    !SCREENSHOTS,
    "set VOXA_SCREENSHOTS=1 (bun run screenshots) to write docs/screenshots",
  );
  test.use({ deviceScaleFactor: 1, viewport: { width: 1200, height: 900 } });

  // The capsule is small: shoot it at 2x so the glass details are visible.
  test.describe("capsule close-ups", () => {
    test.use({ deviceScaleFactor: 2 });
    overlayShots();
  });

  function overlayShots() {
    test("overlay states", async ({ page }) => {
      await page.goto(overlayUrl());
      await page.waitForTimeout(600); // let the pop-in settle
      for (const [id] of STAGES) {
        await shot(
          page.locator(`[data-testid="stage-${id}"]`),
          `overlay-${id}`,
        );
      }
      for (const id of ["dark-recording", "light-recording"]) {
        const stage = page.locator(`[data-testid="stage-${id}"]`);
        await stage.locator(".vg-pill").hover();
        await page.waitForTimeout(400);
        await shot(stage, `overlay-${id}-hover`);
        await page.mouse.move(0, 0);
      }
      await shot(page.locator(".pv-gallery"), "overlay-gallery");
    });

    test("overlay with reduced transparency and reduced motion", async ({
      page,
    }) => {
      const client = await page.context().newCDPSession(page);
      await client.send("Emulation.setEmulatedMedia", {
        features: [
          { name: "prefers-reduced-transparency", value: "reduce" },
          { name: "prefers-reduced-motion", value: "reduce" },
        ],
      });
      await page.goto(overlayUrl("&only=dark-recording"));
      await page.waitForTimeout(400);
      await shot(
        page.locator('[data-testid="stage-dark-recording"]'),
        "overlay-dark-recording-reduced",
      );
      await page.goto(overlayUrl("&only=light-transcribing"));
      await page.waitForTimeout(400);
      await shot(
        page.locator('[data-testid="stage-light-transcribing"]'),
        "overlay-light-transcribing-reduced",
      );
    });
  }

  for (const theme of ["light", "dark"] as const) {
    test(`app screens, ${theme}`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: theme });
      await page.setViewportSize({ width: 900, height: 640 });
      await page.goto(`/voxa-preview?view=app&theme=${theme}&lang=ru`);
      await page.waitForTimeout(500);
      await page.screenshot({
        path: path.join(OUT, `app-general-${theme}.png`),
      });
      await page.getByText("Статистика", { exact: true }).first().click();
      await page.waitForTimeout(500);
      await page.screenshot({
        path: path.join(OUT, `app-statistics-${theme}-top.png`),
      });
    });

    test(`statistics, full height, ${theme}`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: theme });
      await page.setViewportSize({ width: 600, height: 1500 });
      await page.goto(`/voxa-preview?view=stats&theme=${theme}&lang=ru`);
      await page.waitForTimeout(500);
      await page
        .locator(".vg-stats")
        .screenshot({ path: path.join(OUT, `statistics-${theme}.png`) });
    });
  }

  test("statistics, empty", async ({ page }) => {
    await page.setViewportSize({ width: 900, height: 640 });
    await page.goto("/voxa-preview?view=app&theme=dark&lang=ru&data=empty");
    await page.getByText("Статистика", { exact: true }).first().click();
    await page.waitForTimeout(500);
    await page.screenshot({
      path: path.join(OUT, "statistics-empty-dark.png"),
    });
  });
});
