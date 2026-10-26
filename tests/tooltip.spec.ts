import { test, expect, type Page } from "@playwright/test";

const info = (page: Page) =>
  page
    .getByTestId("primary-help")
    .getByRole("button", { name: "More information" });
const description = (lng = "en") =>
  lng === "ko"
    ? "오디오 피드백 사운드의 볼륨 조절"
    : "Adjust the volume of audio feedback sounds";
const primaryTip = (page: Page, lng = "en") =>
  page.getByRole("tooltip").filter({ hasText: description(lng) });
const open = async (page: Page, query = "") => {
  await page.goto(`/tests/fixtures/tooltip.html${query}`);
  await expect(info(page)).toBeVisible();
};
const focusInfo = async (page: Page) => {
  await page.keyboard.press("Tab");
  await expect(info(page)).toBeFocused();
};

for (const lng of ["en", "ko"]) {
  for (const layout of ["horizontal", "stacked"]) {
    test(`focused setting tooltip dismisses on Escape: ${lng} ${layout}`, async ({
      page,
    }) => {
      await open(
        page,
        `?lng=${lng}&layout=${layout}${layout === "stacked" ? "&grouped" : ""}`,
      );
      await focusInfo(page);
      const tip = primaryTip(page, lng);
      await expect(tip).toBeVisible();
      await expect(info(page)).toHaveAttribute(
        "aria-describedby",
        (await tip.getAttribute("id")) ?? "",
      );

      await info(page).press("Escape");

      await expect(tip).toHaveCount(0);
      await expect(info(page)).toBeFocused();
      await expect(info(page)).not.toHaveAttribute("aria-describedby");
      await expect(page.getByTestId("outer-escapes")).toHaveText("0");
    });
  }
}

test("the first Escape dismisses the tooltip before its Dialog", async ({
  page,
}) => {
  await open(page);
  const opener = page.getByTestId("open-dialog");
  await opener.click();
  const dialog = page.getByRole("dialog", { name: "Tooltip test" });
  await expect(dialog).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(
    dialog.getByRole("button", { name: "Close dialog" }),
  ).toBeFocused();
  await page.keyboard.press("Tab");
  const trigger = page
    .getByTestId("dialog-help")
    .getByRole("button", { name: "More information" });
  await expect(trigger).toBeFocused();
  await expect(primaryTip(page)).toBeVisible();

  await trigger.press("Escape");

  await expect(primaryTip(page)).toHaveCount(0);
  await expect(dialog).toBeVisible();
  await expect(trigger).toBeFocused();

  await trigger.press("Escape");

  await expect(dialog).toHaveCount(0);
  await expect(opener).toBeFocused();
});

test("Escape on a dismissed tooltip remains available to its parent", async ({
  page,
}) => {
  await open(page);
  await focusInfo(page);
  await info(page).press("Escape");
  await expect(primaryTip(page)).toHaveCount(0);
  await info(page).press("Escape");
  await expect(page.getByTestId("outer-escapes")).toHaveText("1");
});

test("refocusing reopens the description after Escape", async ({ page }) => {
  await open(page);
  await focusInfo(page);
  await info(page).press("Escape");
  await expect(primaryTip(page)).toHaveCount(0);
  await page.keyboard.press("Tab");
  await page.keyboard.press("Shift+Tab");
  await expect(info(page)).toBeFocused();
  await expect(primaryTip(page)).toBeVisible();
  await expect(info(page)).toHaveAttribute(
    "aria-describedby",
    (await primaryTip(page).getAttribute("id")) ?? "",
  );
});

test("hovered and focused help stays dismissed with the pointer still over it", async ({
  page,
}) => {
  await open(page);
  await info(page).hover();
  await expect(primaryTip(page)).toBeVisible();
  await focusInfo(page);
  await info(page).press("Escape");
  await expect(primaryTip(page)).toHaveCount(0);
  // Wait beyond the existing 380ms hover timer to catch a delayed reopening.
  await page.waitForTimeout(450);
  await expect(primaryTip(page)).toHaveCount(0);
  await expect(info(page)).toBeFocused();
});

test("mouse leave still dismisses a hovered description", async ({ page }) => {
  await open(page);
  await info(page).hover();
  await expect(primaryTip(page)).toBeVisible();
  await page.mouse.move(0, 0);
  await expect(primaryTip(page)).toHaveCount(0);
});

test("pointer dismissal keeps Escape available to the parent", async ({
  page,
}) => {
  await open(page);
  await focusInfo(page);
  await expect(primaryTip(page)).toBeVisible();
  await info(page).click();
  await expect(primaryTip(page)).toHaveCount(0);
  await info(page).press("Escape");
  await expect(page.getByTestId("outer-escapes")).toHaveText("1");
});

test("Escape only dismisses the focused tooltip", async ({ page }) => {
  await open(page);
  const sibling = page
    .getByRole("tooltip")
    .filter({ hasText: "Sibling description" });
  await page.getByTestId("sibling-trigger").hover();
  await expect(sibling).toBeVisible();
  await focusInfo(page);
  await expect(primaryTip(page)).toBeVisible();

  await info(page).press("Escape");

  await expect(primaryTip(page)).toHaveCount(0);
  await expect(sibling).toBeVisible();
  await expect(info(page)).toBeFocused();
});

for (const marker of [
  { name: "isComposing", isComposing: true, keyCode: 27 },
  { name: "legacy229", isComposing: false, keyCode: 229 },
]) {
  test(`IME Escape is not consumed by the tooltip: ${marker.name}`, async ({
    page,
  }) => {
    await open(page);
    const input = page.getByRole("textbox", { name: "IME input" });
    const tip = page.getByRole("tooltip").filter({ hasText: "Input hint" });
    await input.focus();
    await expect(tip).toBeVisible();
    const prevented = await input.evaluate(
      (element, init) => {
        const event = new KeyboardEvent("keydown", {
          key: "Escape",
          ...init,
          bubbles: true,
          cancelable: true,
        });
        element.dispatchEvent(event);
        return event.defaultPrevented;
      },
      { isComposing: marker.isComposing, keyCode: marker.keyCode },
    );
    expect(prevented).toBe(false);
    await expect(tip).toBeVisible();
    await expect(input).toHaveValue("한글");
  });
}

test("a child's handled Escape is not consumed again", async ({ page }) => {
  await open(page, "?child-handles-escape");
  const input = page.getByRole("textbox", { name: "IME input" });
  const tip = page.getByRole("tooltip").filter({ hasText: "Input hint" });
  await input.focus();
  await expect(tip).toBeVisible();
  await input.press("Escape");
  await expect(tip).toBeVisible();
  await expect(input).toBeFocused();
});

test("reopening retains tooltip placement near the viewport edge", async ({
  page,
}) => {
  await open(page, "?edge");
  await focusInfo(page);
  await info(page).press("Escape");
  await expect(primaryTip(page)).toHaveCount(0);
  await page.keyboard.press("Tab");
  await page.keyboard.press("Shift+Tab");
  await expect(primaryTip(page)).toBeVisible();
  await expect
    .poll(async () => {
      const trigger = await info(page).boundingBox();
      const tip = await primaryTip(page).boundingBox();
      return trigger && tip ? tip.y >= trigger.y + trigger.height : false;
    })
    .toBe(true);
});

test("other keys retain the current focus and hover behavior", async ({
  page,
}) => {
  await open(page);
  await focusInfo(page);
  await info(page).press("Enter");
  await info(page).press("Space");
  await expect(primaryTip(page)).toBeVisible();
  await expect(info(page)).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(primaryTip(page)).toHaveCount(0);
});
