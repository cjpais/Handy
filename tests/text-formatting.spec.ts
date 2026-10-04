import { test, expect } from "@playwright/test";

test("replacement drafts survive option changes and save; duplicates are rejected", async ({
  page,
}) => {
  await page.goto("/tests/text-formatting.html");
  await page
    .getByRole("textbox", { name: "Spoken phrase 1", exact: true })
    .fill("exclamation mark");
  await page.getByText("Lowercase", { exact: true }).click();
  await page.getByText("Uppercase", { exact: true }).click();
  await expect(
    page.getByRole("textbox", { name: "Spoken phrase 1", exact: true }),
  ).toHaveValue("exclamation mark");
  await page.getByRole("button", { name: "Save replacements" }).click();
  await expect(page.getByRole("alert")).toHaveCount(0);
  await page.getByRole("button", { name: "Add replacement" }).click();
  await page
    .getByRole("textbox", { name: "Spoken phrase 3", exact: true })
    .fill("EXCLAMATION MARK");
  await page.getByRole("button", { name: "Save replacements" }).click();
  await expect(page.getByRole("alert")).toBeVisible();
  await page
    .getByRole("button", { name: "Remove replacement 3", exact: true })
    .click();
  await page.getByRole("button", { name: "Save replacements" }).click();
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(
    page.getByRole("textbox", { name: "Replacement 2", exact: true }),
  ).toHaveValue("\\n");
});

test("all period choices and disabling formatting are available", async ({
  page,
}) => {
  await page.goto("/tests/text-formatting.html");
  await page.getByText("Keep original", { exact: true }).click();
  await page.getByText("Remove final period", { exact: true }).click();
  await expect(
    page.getByText("Remove final period", { exact: true }),
  ).toBeVisible();
  await page.getByText("Remove final period", { exact: true }).click();
  await page.getByText("Remove sentence periods", { exact: true }).click();
  await page.getByText("Remove sentence periods", { exact: true }).click();
  await page.getByText("Only spoken periods", { exact: true }).click();
  await expect(
    page.getByText("Only spoken periods", { exact: true }),
  ).toBeVisible();
  await page.getByText("Lowercase", { exact: true }).click();
  await page
    .getByText("Capitalize at line starts and sentence boundaries", {
      exact: true,
    })
    .click();
  await expect(
    page.getByText("Capitalize at line starts and sentence boundaries", {
      exact: true,
    }),
  ).toBeVisible();
  await page.locator('input[type="checkbox"]').first().uncheck({ force: true });
  await expect(page.getByRole("textbox")).toHaveCount(0);
});
