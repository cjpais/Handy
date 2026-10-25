import { test, expect, type Locator } from "@playwright/test";

const trigger = (menu: Locator) => menu.locator("button[aria-expanded]");

test.beforeEach(async ({ page }) => {
  await page.goto("/tests/fixtures/dropdown.html");
  await expect(page.getByTestId("default-menu")).toBeVisible({
    timeout: 10000,
  });
});

test("Escape closes the open menu without selecting", async ({ page }) => {
  const button = trigger(page.getByTestId("default-menu"));
  await page.keyboard.press("Tab");
  await expect(button).toBeFocused();
  await button.press("Enter");
  await expect(button).toHaveAttribute("aria-expanded", "true");

  await button.press("Escape");

  await expect(button).toHaveAttribute("aria-expanded", "false");
  await expect(button).toBeFocused();
  await expect(page.getByTestId("selection")).toHaveText("one");
  await expect(page.getByTestId("selections")).toHaveText("0");
  await expect(page.getByTestId("outer-escapes")).toHaveText("0");
});

test("Escape from an option restores focus to its trigger", async ({
  page,
}) => {
  const menu = page.getByTestId("default-menu");
  const button = trigger(menu);
  await button.press("Enter");
  await page.keyboard.press("Tab");
  await expect(
    menu.locator("button:not([aria-expanded])").first(),
  ).toBeFocused();

  await page.keyboard.press("Escape");

  await expect(button).toHaveAttribute("aria-expanded", "false");
  await expect(button).toBeFocused();
  await expect(page.getByTestId("outer-escapes")).toHaveText("0");
});

test("Escape on a closed menu remains available to its parent", async ({
  page,
}) => {
  const button = trigger(page.getByTestId("default-menu"));
  await button.focus();
  await button.press("Escape");

  await expect(button).toHaveAttribute("aria-expanded", "false");
  await expect(button).toBeFocused();
  await expect(page.getByTestId("opens")).toHaveText("0");
  await expect(page.getByTestId("outer-escapes")).toHaveText("1");
});

test("Escape affects only the dropdown handling the key", async ({ page }) => {
  const first = trigger(page.getByTestId("default-menu"));
  const second = trigger(page.getByTestId("above-menu"));
  await first.press("Enter");
  await second.focus();
  await second.press("Enter");
  await expect(first).toHaveAttribute("aria-expanded", "true");
  await expect(second).toHaveAttribute("aria-expanded", "true");

  await second.press("Escape");

  await expect(first).toHaveAttribute("aria-expanded", "true");
  await expect(second).toHaveAttribute("aria-expanded", "false");
  await expect(second).toBeFocused();
  await expect(page.getByTestId("outer-escapes")).toHaveText("0");
});

test("the menu reopens and still selects after Escape", async ({ page }) => {
  const menu = page.getByTestId("default-menu");
  const button = trigger(menu);
  await button.press("Enter");
  await button.press("Escape");
  await button.press("Space");
  await menu
    .getByRole("button", { name: "Second choice", exact: true })
    .press("Enter");

  await expect(button).toHaveAttribute("aria-expanded", "false");
  await expect(button).toHaveText("Second choice");
  await expect(page.getByTestId("selection")).toHaveText("two");
  await expect(page.getByTestId("opens")).toHaveText("2");
  await expect(page.getByTestId("selections")).toHaveText("1");
});

test("above placement is preserved while Escape restores focus", async ({
  page,
}) => {
  const menu = page.getByTestId("above-menu");
  const button = trigger(menu);
  await button.press("Enter");
  const option = menu.getByRole("button", {
    name: "Above second",
    exact: true,
  });
  const optionBox = await option.boundingBox();
  const triggerBox = await button.boundingBox();
  expect(optionBox).not.toBeNull();
  expect(triggerBox).not.toBeNull();
  expect(optionBox!.y + optionBox!.height).toBeLessThanOrEqual(triggerBox!.y);

  await option.press("Escape");

  await expect(button).toHaveAttribute("aria-expanded", "false");
  await expect(button).toBeFocused();
});

test("an empty menu can be dismissed with Escape", async ({ page }) => {
  const menu = page.getByTestId("empty-menu");
  const button = trigger(menu);
  await button.press("Enter");
  await expect(
    menu.getByText("No options found", { exact: true }),
  ).toBeVisible();

  await button.press("Escape");

  await expect(button).toHaveAttribute("aria-expanded", "false");
  await expect(button).toBeFocused();
  await expect(menu.getByText("No options found", { exact: true })).toHaveCount(
    0,
  );
});

test("the first Escape dismisses the menu and the next dismisses its Dialog", async ({
  page,
}) => {
  const openDialog = page.getByTestId("open-dialog");
  await openDialog.click();
  const dialog = page.getByRole("dialog", { name: "Dropdown keyboard test" });
  await expect(dialog).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(
    dialog.getByRole("button", { name: "Close dialog" }),
  ).toBeFocused();
  await page.keyboard.press("Tab");
  const button = trigger(page.getByTestId("dialog-menu"));
  await expect(button).toBeFocused();
  await button.press("Enter");
  await page.keyboard.press("Tab");
  await expect(
    dialog.getByRole("button", { name: "First choice", exact: true }).last(),
  ).toBeFocused();

  await page.keyboard.press("Escape");

  await expect(dialog).toBeVisible();
  await expect(button).toHaveAttribute("aria-expanded", "false");
  await expect(button).toBeFocused();

  await page.keyboard.press("Escape");

  await expect(dialog).toHaveCount(0);
  await expect(openDialog).toBeFocused();
});
