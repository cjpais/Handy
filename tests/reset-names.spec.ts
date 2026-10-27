import { test, expect, type Page } from "@playwright/test";

// Replace native settings/capture services; render the real settings components,
// ResetButton, and locale resources in the browser.
test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    Object.assign(window, {
      resetFixture: {
        resets: [] as string[],
        bindings: Object.fromEntries(
          [
            "transcribe",
            "cancel",
            "transcribe_with_post_process",
            "custom",
          ].map((id) => [
            id,
            {
              name: "Custom action",
              description: "Custom binding",
              current_binding: "ctrl+space",
            },
          ]),
        ),
      },
    });
  });
  await page.route("**/src/hooks/useSettings.ts*", (route) =>
    route.fulfill({
      contentType: "application/javascript",
      body: `export const useSettings = () => ({
        getSetting: key => key === "bindings" ? window.resetFixture.bindings : null,
        resetBinding: id => window.resetFixture.resets.push(id),
        isUpdating: key => new URLSearchParams(location.search).has("busy") && key === "binding_transcribe",
        isLoading: false,
      });`,
    }),
  );
  await page.route("**/src/hooks/useOsType.ts*", (route) =>
    route.fulfill({
      contentType: "application/javascript",
      body: 'export const useOsType = () => "windows";',
    }),
  );
  await page.route("**/src/hooks/useShortcutCapture.ts*", (route) =>
    route.fulfill({
      contentType: "application/javascript",
      body: "export const useShortcutCapture = () => ({capturing: false, partial: '', start() {}, cancel() {}});",
    }),
  );
});

const openFixture = async (page: Page, query = "") => {
  await page.goto(`/tests/fixtures/reset-names.html${query}`);
  await expect(page.getByTestId("transcribe")).toBeVisible();
};

for (const [language, names] of [
  [
    "en",
    [
      "Reset Transcribe Shortcut to default",
      "Reset Cancel Shortcut to default",
      "Reset Post-Processing Hotkey to default",
    ],
  ],
  [
    "ko",
    [
      "음성 텍스트 변환 단축키 기본값으로 초기화",
      "취소 단축키 기본값으로 초기화",
      "후처리 단축키 기본값으로 초기화",
    ],
  ],
] as const) {
  test(`shortcut reset buttons identify their settings in ${language}`, async ({
    page,
  }) => {
    await openFixture(page, `?lng=${language}`);
    for (const name of names) {
      await expect(
        page.getByRole("button", { name, exact: true }),
      ).toBeVisible();
    }
  });
}

test("reset names update when the app language changes", async ({ page }) => {
  await openFixture(page);
  await expect(
    page.getByRole("button", { name: "Reset Transcribe Shortcut to default" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Switch to Korean" }).click();
  await expect(
    page.getByRole("button", {
      name: "음성 텍스트 변환 단축키 기본값으로 초기화",
    }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "볼륨 기본값으로 초기화" }),
  ).toBeVisible();
});

test("named reset buttons retain their keyboard action", async ({ page }) => {
  await openFixture(page);
  const cancel = page.getByRole("button", {
    name: "Reset Cancel Shortcut to default",
  });
  await cancel.focus();
  await page.keyboard.press("Enter");
  await expect
    .poll(() => page.evaluate("window.resetFixture.resets"))
    .toEqual(["cancel"]);
});

test("custom shortcut names retain their fallback", async ({ page }) => {
  await openFixture(page);
  await expect(
    page
      .getByTestId("custom")
      .getByRole("button", { name: "Reset Custom action to default" }),
  ).toBeVisible();
});

test("the slider reset identifies and resets the volume", async ({ page }) => {
  await openFixture(page);
  await page.getByRole("button", { name: "Reset Volume to default" }).click();
  await expect(page.getByRole("slider", { name: "Volume" })).toHaveValue("0.5");
  await expect(page.getByTestId("volume-resets")).toHaveText("1");
});

test("updating settings keep named reset buttons disabled", async ({
  page,
}) => {
  await openFixture(page, "?busy");
  await expect(
    page.getByRole("button", { name: "Reset Transcribe Shortcut to default" }),
  ).toBeDisabled();
  await expect(
    page.getByRole("button", { name: "Reset Volume to default" }),
  ).toBeDisabled();
  await expect(
    page.getByRole("button", { name: "Reset Cancel Shortcut to default" }),
  ).toBeEnabled();
});
