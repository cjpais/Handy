import { expect, test, type Page } from "@playwright/test";
import type {} from "./fixtures/audio-player";

async function openPlayer(page: Page, strict = false) {
  await page.goto("/tests/fixtures/audio-player.html");
  await page.waitForFunction(() => window.audioPlayerFixture !== undefined);
  await page.evaluate(
    (strict) => window.audioPlayerFixture.mount({ strict }),
    strict,
  );
}

async function expectReleased(page: Page, count: number) {
  await expect
    .poll(() => page.evaluate(() => window.audioPlayerFixture.revoked.length))
    .toBe(count);
  expect(
    await page.evaluate(async () => {
      const { created, revoked } = window.audioPlayerFixture;
      const readable = await Promise.all(
        created.map(async (url) => {
          try {
            await fetch(url);
            return true;
          } catch {
            return false;
          }
        }),
      );
      return {
        created: created.length,
        uniqueRevocations: new Set(revoked).size,
        stillReadable: readable.some(Boolean),
      };
    }),
  ).toEqual({ created: count, uniqueRevocations: count, stillReadable: false });
}

for (const strict of [false, true]) {
  test.describe(strict ? "StrictMode lazy audio" : "lazy audio", () => {
    test("releases a source arriving after the player is removed", async ({
      page,
    }) => {
      await openPlayer(page, strict);
      await page.getByRole("button", { name: "Play", exact: true }).click();
      await page.evaluate(() => window.audioPlayerFixture.unmount());
      await page.evaluate(() => window.audioPlayerFixture.resolve(0));
      await expectReleased(page, 1);
      await expect(page.locator("audio")).toHaveCount(0);
    });

    test("releases a resolved source even before its state commits", async ({
      page,
    }) => {
      await openPlayer(page, strict);
      await page.getByRole("button", { name: "Play", exact: true }).click();
      await page.evaluate(() =>
        window.audioPlayerFixture.resolve(0, "blob", true),
      );
      await expectReleased(page, 1);
      await expect(page.locator("audio")).toHaveCount(0);
    });

    test("a late source cannot attach to a newly mounted player", async ({
      page,
    }) => {
      await openPlayer(page, strict);
      await page.getByRole("button", { name: "Play", exact: true }).click();
      await page.evaluate(
        (strict) => window.audioPlayerFixture.mount({ strict }),
        strict,
      );
      await page.evaluate(() => window.audioPlayerFixture.resolve(0));
      await expectReleased(page, 1);
      await expect(page.locator("audio")).not.toHaveAttribute("src");
      await expect(
        page.getByRole("button", { name: "Play", exact: true }),
      ).toBeEnabled();
    });

    test("normal playback keeps its source until unmount", async ({ page }) => {
      await openPlayer(page, strict);
      await page.getByRole("button", { name: "Play", exact: true }).click();
      await page.evaluate(() => window.audioPlayerFixture.resolve(0));
      await expect(
        page.getByRole("button", { name: "Pause", exact: true }),
      ).toBeVisible();
      expect(
        await page.evaluate(() => window.audioPlayerFixture.revoked),
      ).toEqual([]);
      expect(
        await page.evaluate(
          async () => (await fetch(window.audioPlayerFixture.created[0])).ok,
        ),
      ).toBe(true);
      await page.getByRole("button", { name: "Pause", exact: true }).click();
      await page.getByRole("button", { name: "Play", exact: true }).click();
      await expect(
        page.getByRole("button", { name: "Pause", exact: true }),
      ).toBeVisible();
      expect(
        await page.evaluate(() => window.audioPlayerFixture.requests.length),
      ).toBe(1);
      await page.evaluate(() => window.audioPlayerFixture.unmount());
      await expectReleased(page, 1);
    });
  });
}

test("repeated play-and-leave cycles release every late source", async ({
  page,
}) => {
  await openPlayer(page);
  for (let index = 0; index < 3; index++) {
    if (index > 0) await page.evaluate(() => window.audioPlayerFixture.mount());
    await page.getByRole("button", { name: "Play", exact: true }).click();
    await page.evaluate(() => window.audioPlayerFixture.unmount());
    await page.evaluate(
      (index) => window.audioPlayerFixture.resolve(index),
      index,
    );
    await expectReleased(page, index + 1);
  }
});

test("null loads can be retried", async ({ page }) => {
  await openPlayer(page);
  await page.getByRole("button", { name: "Play", exact: true }).click();
  await page.evaluate(() => window.audioPlayerFixture.resolve(0, "null"));
  await expect(
    page.getByRole("button", { name: "Play", exact: true }),
  ).toBeEnabled();
  await page.getByRole("button", { name: "Play", exact: true }).click();
  await page.evaluate(() => window.audioPlayerFixture.resolve(1));
  await expect(
    page.getByRole("button", { name: "Pause", exact: true }),
  ).toBeVisible();
  await page.evaluate(() => window.audioPlayerFixture.unmount());
  await expectReleased(page, 1);
});

test("rejected loads clear the loading state and permit retry", async ({
  page,
}) => {
  await openPlayer(page);
  await page.getByRole("button", { name: "Play", exact: true }).click();
  await page.evaluate(() => window.audioPlayerFixture.reject(0));
  await expect(
    page.getByRole("button", { name: "Play", exact: true }),
  ).toBeEnabled();
  await page.getByRole("button", { name: "Play", exact: true }).click();
  await page.evaluate(() => window.audioPlayerFixture.resolve(1));
  await expect(
    page.getByRole("button", { name: "Pause", exact: true }),
  ).toBeVisible();
  await page.evaluate(() => window.audioPlayerFixture.unmount());
  await expectReleased(page, 1);
});

test("a rejected load after unmount creates no resources", async ({ page }) => {
  await openPlayer(page);
  await page.getByRole("button", { name: "Play", exact: true }).click();
  await page.evaluate(() => window.audioPlayerFixture.unmount());
  await page.evaluate(() => window.audioPlayerFixture.reject(0));
  await expectReleased(page, 0);
});

test("late non-Blob sources are not revoked", async ({ page }) => {
  await openPlayer(page);
  await page.getByRole("button", { name: "Play", exact: true }).click();
  await page.evaluate(() => window.audioPlayerFixture.unmount());
  await page.evaluate(() => window.audioPlayerFixture.resolve(0, "asset"));
  await expectReleased(page, 0);
});

test("an initially supplied Blob is released on unmount", async ({ page }) => {
  await openPlayer(page);
  await page.evaluate(() =>
    window.audioPlayerFixture.mount({ initialBlob: true }),
  );
  await expect(page.locator("audio")).toHaveAttribute("src", /^blob:/);
  await page.evaluate(() => window.audioPlayerFixture.unmount());
  await expectReleased(page, 1);
});

test("group playback still pauses the other player", async ({ page }) => {
  await openPlayer(page);
  await page.evaluate(() => window.audioPlayerFixture.mount({ players: 2 }));
  const first = page.getByTestId("player-0");
  const second = page.getByTestId("player-1");
  await first.getByRole("button", { name: "Play", exact: true }).click();
  await page.evaluate(() => window.audioPlayerFixture.resolve(0));
  await expect(
    first.getByRole("button", { name: "Pause", exact: true }),
  ).toBeVisible();
  await second.getByRole("button", { name: "Play", exact: true }).click();
  await page.evaluate(() => window.audioPlayerFixture.resolve(1));
  await expect(
    second.getByRole("button", { name: "Pause", exact: true }),
  ).toBeVisible();
  await expect(
    first.getByRole("button", { name: "Play", exact: true }),
  ).toBeVisible();
  expect(await page.evaluate(() => window.audioPlayerFixture.revoked)).toEqual(
    [],
  );
  await page.evaluate(() => window.audioPlayerFixture.unmount());
  await expectReleased(page, 2);
});
