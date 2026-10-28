import { test, expect, type Locator, type Page } from "@playwright/test";

// Replace native settings, IPC and events. Input handlers, picker/table and
// language/detector hooks, localization and keyboard events are real browser code.
test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    Object.assign(window, {
      imeFixture: {
        settings: {
          custom_words: [],
          selected_language: "en",
          spoken_languages: ["en"],
        },
        writes: [],
      },
    });
  });
  await page.route("**/src/hooks/useSettings.ts*", (route) =>
    route.fulfill({
      contentType: "application/javascript",
      body: `export const useSettings = () => ({
      getSetting: key => window.imeFixture.settings[key],
      updateSetting: async (key,value) => {
        window.imeFixture.writes.push({key,value});
        window.imeFixture.settings[key]=value;
      },
      resetSetting: async () => {}, isUpdating: () => false,
    });`,
    }),
  );
});

const open = async (page: Page, view: string) => {
  await page.goto(`/tests/fixtures/ime.html?view=${view}`);
  await expect(page.getByTestId("ready")).toBeVisible();
};
const writes = (page: Page) => page.evaluate("window.imeFixture.writes");
const markers = [
  { name: "isComposing", isComposing: true, keyCode: 13 },
  { name: "legacy229", isComposing: false, keyCode: 229 },
] as const;
const keyDuringComposition = async (
  input: Locator,
  key: string,
  marker: (typeof markers)[number],
) => {
  await input.dispatchEvent("compositionstart", { data: "" });
  await input.dispatchEvent("compositionupdate", { data: "한글" });
  // KeyboardEvent defaults are explicit so preventDefault can be checked too.
  return input.evaluate(
    (element, init) => {
      const event = new KeyboardEvent("keydown", {
        ...init,
        bubbles: true,
        cancelable: true,
      });
      element.dispatchEvent(event);
      return event.defaultPrevented;
    },
    {
      key,
      isComposing: marker.isComposing,
      keyCode:
        marker.keyCode === 229
          ? 229
          : ({ Enter: 13, Escape: 27, ArrowUp: 38, ArrowDown: 40 }[key] ?? 0),
    },
  );
};
const finishComposition = (input: Locator) =>
  input.dispatchEvent("compositionend", { data: "한글" });

for (const marker of markers) {
  test(`custom-word confirmation leaves text intact: ${marker.name}`, async ({
    page,
  }) => {
    await open(page, "words");
    const input = page.getByRole("textbox");
    await input.fill("한글");
    expect(await keyDuringComposition(input, "Enter", marker)).toBe(false);
    await expect(input).toHaveValue("한글");
    expect(await writes(page)).toEqual([]);
    await finishComposition(input);
    await input.press("Enter");
    await expect
      .poll(() => writes(page))
      .toEqual([{ key: "custom_words", value: ["한글"] }]);
    await expect(input).toHaveValue("");
  });

  test(`recognition-language confirmation does not select: ${marker.name}`, async ({
    page,
  }) => {
    await open(page, "language");
    await page.locator("button").first().click();
    const input = page.getByRole("textbox");
    await input.fill("한국");
    expect(await keyDuringComposition(input, "Enter", marker)).toBe(false);
    await expect(input).toBeVisible();
    await expect(input).toHaveValue("한국");
    expect(await writes(page)).toEqual([]);
    await finishComposition(input);
    await input.press("Enter");
    await expect
      .poll(() => writes(page))
      .toEqual([{ key: "selected_language", value: "ko" }]);
    await expect(input).toHaveCount(0);
  });

  test(`model-search confirmation does not activate a row: ${marker.name}`, async ({
    page,
  }) => {
    await open(page, "model");
    const input = page.getByRole("searchbox");
    await input.fill("한글");
    await input.press("ArrowDown");
    await expect(page.getByRole("radio", { name: "Beta" })).toHaveAttribute(
      "data-highlighted",
      "true",
    );
    expect(await keyDuringComposition(input, "Enter", marker)).toBe(false);
    await expect(page.getByTestId("selected-models")).toHaveText("[]");
    await finishComposition(input);
    await input.press("Enter");
    await expect(page.getByTestId("selected-models")).toHaveText('["beta"]');
  });

  test(`model-search IME navigation keeps its highlight: ${marker.name}`, async ({
    page,
  }) => {
    await open(page, "model");
    const input = page.getByRole("searchbox");
    await input.press("ArrowDown");
    const beta = page.getByRole("radio", { name: "Beta" });
    expect(await keyDuringComposition(input, "ArrowUp", marker)).toBe(false);
    await expect(beta).toHaveAttribute("data-highlighted", "true");
    await finishComposition(input);
    await input.press("ArrowUp");
    await expect(page.getByRole("radio", { name: "Alpha" })).toHaveAttribute(
      "data-highlighted",
      "true",
    );
  });

  test(`model-search IME Escape preserves query and page: ${marker.name}`, async ({
    page,
  }) => {
    await open(page, "model");
    const input = page.getByRole("searchbox");
    await input.fill("한글");
    expect(await keyDuringComposition(input, "Escape", marker)).toBe(false);
    await expect(input).toHaveValue("한글");
    await expect(page.getByTestId("model-escapes")).toHaveText("0");
    await finishComposition(input);
    await input.press("Escape");
    await expect(input).toHaveValue("");
    await input.press("Escape");
    await expect(page.getByTestId("model-escapes")).toHaveText("1");
  });

  test(`preferred-language confirmation does not add: ${marker.name}`, async ({
    page,
  }) => {
    await open(page, "preferred");
    const input = page.getByRole("textbox");
    await input.fill("한국");
    expect(await keyDuringComposition(input, "Enter", marker)).toBe(false);
    await expect(input).toHaveValue("한국");
    expect(await writes(page)).toEqual([]);
    await finishComposition(input);
    await input.press("Enter");
    await expect
      .poll(() => writes(page))
      .toEqual([{ key: "spoken_languages", value: ["en", "ko"] }]);
  });

  test(`preferred-language IME Escape retains focus: ${marker.name}`, async ({
    page,
  }) => {
    await open(page, "preferred");
    const input = page.getByRole("textbox");
    await input.fill("한국");
    expect(await keyDuringComposition(input, "Escape", marker)).toBe(false);
    await expect(input).toBeFocused();
    await expect(input).toHaveValue("한국");
    await finishComposition(input);
    await input.press("Escape");
    await expect(input).toHaveValue("");
    await input.press("Escape");
    await expect(input).not.toBeFocused();
  });

  test(`preferred-language IME Backspace does not remove a chip: ${marker.name}`, async ({
    page,
  }) => {
    await open(page, "preferred");
    await page.evaluate(
      'window.imeFixture.settings.spoken_languages = ["en","ko"]',
    );
    const input = page.getByRole("textbox");
    // An input state change refreshes the fixture's external settings snapshot.
    await input.fill("x");
    await input.fill("");
    expect(await keyDuringComposition(input, "Backspace", marker)).toBe(false);
    expect(await writes(page)).toEqual([]);
    await finishComposition(input);
    await input.press("Backspace");
    await expect
      .poll(() => writes(page))
      .toEqual([{ key: "spoken_languages", value: ["en"] }]);
  });
}

test("an IME boundary Enter after compositionend does not submit the word", async ({
  page,
}) => {
  await open(page, "words");
  const input = page.getByRole("textbox");
  await input.fill("한글");
  await input.dispatchEvent("compositionstart", { data: "" });
  await input.dispatchEvent("compositionupdate", { data: "한글" });
  await input.dispatchEvent("compositionend", { data: "한글" });
  await input.dispatchEvent("keydown", {
    key: "Enter",
    isComposing: false,
    keyCode: 229,
  });
  expect(await writes(page)).toEqual([]);
  await expect(input).toHaveValue("한글");
  await input.press("Enter");
  await expect
    .poll(() => writes(page))
    .toEqual([{ key: "custom_words", value: ["한글"] }]);
});

test("recognition-language IME Escape keeps the picker open", async ({
  page,
}) => {
  await open(page, "language");
  await page.locator("button").first().click();
  const input = page.getByRole("textbox");
  await input.fill("한국");
  expect(await keyDuringComposition(input, "Escape", markers[0])).toBe(false);
  await expect(input).toBeVisible();
  await expect(input).toHaveValue("한국");
  await finishComposition(input);
  await input.press("Escape");
  await expect(input).toHaveCount(0);
});

test("model search ignores an ordinary Enter without a highlighted row", async ({
  page,
}) => {
  await open(page, "model");
  await page.getByRole("searchbox").press("Enter");
  await expect(page.getByTestId("selected-models")).toHaveText("[]");
});

for (const marker of markers) {
  test(`recognition-language Enter skips unavailable Auto after composition: ${marker.name}`, async ({
    page,
  }) => {
    await open(page, "language&autoUnavailable=1");
    await page.locator("button").first().click();
    const input = page.getByRole("textbox");
    // Auto is listed first but cannot be selected for this backend policy.
    expect(await keyDuringComposition(input, "Enter", marker)).toBe(false);
    expect(await writes(page)).toEqual([]);
    await expect(input).toBeVisible();
    await finishComposition(input);
    await input.press("Enter");
    await expect
      .poll(() => writes(page))
      .toEqual([{ key: "selected_language", value: "en" }]);
    await expect(input).toHaveCount(0);
  });
}
