import { expect, test, type Page } from "@playwright/test";

type FixtureWindow = Window & {
  typingToolFixture: {
    resolveTools: (tools: string[]) => void;
    writes: string[];
    selected: () => string;
  };
};

async function resolveTools(page: Page, tools: string[]) {
  await page.evaluate((available) => {
    (window as FixtureWindow).typingToolFixture.resolveTools(available);
  }, tools);
}

async function savedState(page: Page) {
  return page.evaluate(() => {
    const fixture = (window as FixtureWindow).typingToolFixture;
    return { selected: fixture.selected(), writes: fixture.writes };
  });
}

test("menu contains only the tools returned for the session", async ({
  page,
}) => {
  await page.goto("/tests/fixtures/typing-tool.html");
  const dropdown = page.getByRole("button", {
    name: "Auto (Recommended)",
    exact: true,
  });
  await expect(dropdown).toBeDisabled();
  await resolveTools(page, ["auto", "wtype", "ydotool"]);
  await expect(dropdown).toBeEnabled();
  await dropdown.click();
  await expect(
    page.getByRole("button", { name: "wtype", exact: true }),
  ).toBeEnabled();
  await expect(
    page.getByRole("button", { name: "ydotool", exact: true }),
  ).toBeEnabled();
  for (const tool of ["kwtype", "dotool", "xdotool"]) {
    await expect(
      page.getByRole("button", { name: tool, exact: true }),
    ).toHaveCount(0);
  }
  expect(await savedState(page)).toEqual({ selected: "auto", writes: [] });
});

test("saved unavailable tool remains visible until the user selects Auto", async ({
  page,
}) => {
  await page.goto("/tests/fixtures/typing-tool.html?saved=xdotool");
  const loading = page.getByRole("button", { name: "xdotool", exact: true });
  await expect(loading).toBeDisabled();
  await expect(
    page.getByText("xdotool (unavailable)", { exact: true }),
  ).toHaveCount(0);
  await resolveTools(page, ["auto", "wtype", "ydotool"]);
  const dropdown = page.getByRole("button", {
    name: "xdotool (unavailable)",
    exact: true,
  });
  await expect(dropdown).toBeEnabled();
  expect(await savedState(page)).toEqual({ selected: "xdotool", writes: [] });
  await dropdown.click();
  await expect(
    page.getByRole("button", {
      name: /xdotool \(unavailable\).*Choose Auto or another available typing tool\./,
    }),
  ).toBeDisabled();
  await page
    .getByRole("button", { name: "Auto (Recommended)", exact: true })
    .click();
  await expect(
    page.getByRole("button", { name: "Auto (Recommended)", exact: true }),
  ).toBeEnabled();
  await expect(
    page.getByText("xdotool (unavailable)", { exact: true }),
  ).toHaveCount(0);
  expect(await savedState(page)).toEqual({
    selected: "auto",
    writes: ["auto"],
  });
});
