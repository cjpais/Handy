import { test, expect, type Page } from "@playwright/test";

async function emit(page: Page, event: string, payload: unknown) {
  await page.evaluate(
    ({ event, payload }) => window.audioTest.emit(event, payload),
    { event, payload },
  );
}

async function commandCount(page: Page, command: string) {
  return page.evaluate(
    (command) =>
      window.audioTest.calls.filter((c) => c.command === command).length,
    command,
  );
}

async function expectCommandCount(page: Page, command: string, count: number) {
  await expect.poll(() => commandCount(page, command)).toBe(count);
}

async function startTest(page: Page) {
  const starts = await commandCount(page, "start_microphone_test");
  await page
    .getByRole("button", { name: "Test Microphone", exact: true })
    .click();
  await expectCommandCount(page, "start_microphone_test", starts + 1);
}

// Only IPC is mocked: these tests mount the production React components and
// deliver backend events through Tauri's event implementation. No capture opens.
declare global {
  interface Window {
    audioTest: {
      calls: { command: string; args: { sessionId?: number } }[];
      emit: (event: string, payload: unknown) => Promise<void>;
      resolveStart: (session: number) => void;
      unmount: () => void;
      hide: () => void;
    };
  }
}

test("stopped event before the start response cannot revive a test", async ({
  page,
}) => {
  await page.goto("/tests/fixtures/audio.html?delayed");
  await startTest(page);
  await emit(page, "microphone-test-stopped-event", { session_id: 1 });
  await page.evaluate(() => window.audioTest.resolveStart(1));
  await expect(
    page.getByRole("button", { name: "Test Microphone", exact: true }),
  ).toBeEnabled();
  await expect(page.getByRole("meter")).toHaveCount(0);
});

test("hiding the still-mounted settings window stops capture and clears its meter", async ({
  page,
}) => {
  await page.goto("/tests/fixtures/audio.html");
  await startTest(page);
  await expect(page.getByRole("meter")).toBeVisible();
  await emit(page, "microphone-test-level-event", {
    session_id: 1,
    level: 0.7,
  });
  await expect(page.getByRole("meter")).toHaveAttribute("aria-valuenow", "70");
  await page.screenshot({
    path: test.info().outputPath("microphone-meter.png"),
  });
  await page.evaluate(() => window.audioTest.hide());
  await expectCommandCount(page, "stop_microphone_test", 1);
  await expect(page.getByRole("meter")).toHaveCount(0);
});

test("hide while starting stops the late successful session", async ({
  page,
}) => {
  await page.goto("/tests/fixtures/audio.html?delayed");
  await startTest(page);
  await page.evaluate(() => {
    window.audioTest.hide();
    window.audioTest.resolveStart(1);
  });
  await expectCommandCount(page, "stop_microphone_test", 1);
  await expect(page.getByRole("meter")).toHaveCount(0);
});

test("unmount while starting stops the late successful session", async ({
  page,
}) => {
  await page.goto("/tests/fixtures/audio.html?delayed");
  await startTest(page);
  await page.evaluate(() => {
    window.audioTest.unmount();
    window.audioTest.resolveStart(1);
  });
  await expectCommandCount(page, "stop_microphone_test", 1);
});

test("disconnect clears the last level and stale events cannot affect a newer session", async ({
  page,
}) => {
  await page.goto("/tests/fixtures/audio.html");
  await startTest(page);
  await expect(page.getByRole("meter")).toBeVisible();
  await emit(page, "microphone-test-level-event", {
    session_id: 1,
    level: 0.8,
  });
  await emit(page, "microphone-test-stopped-event", {
    session_id: 1,
    failed: true,
  });
  await expect(page.getByRole("meter")).toHaveCount(0);
  await startTest(page);
  await expect(page.getByRole("meter")).toBeVisible();
  await emit(page, "microphone-test-stopped-event", { session_id: 1 });
  await emit(page, "microphone-test-level-event", {
    session_id: 1,
    level: 0.8,
  });
  await expect(page.getByRole("meter")).toHaveAttribute("aria-valuenow", "0");
});

test("background recording failure reveals a visible warning", async ({
  page,
}) => {
  await page.goto("/tests/fixtures/audio.html?app");
  await expect(
    page.getByRole("button", { name: "Test Microphone", exact: true }),
  ).toBeVisible();
  await expect
    .poll(() => commandCount(page, "get_app_settings"))
    .toBeGreaterThan(0);
  await page.evaluate(() => window.audioTest.hide());
  await emit(page, "recording-error", { error_type: "silent_input" });
  await expectCommandCount(page, "show_main_window_command", 1);
  await expect(
    page.getByText("No Audio Detected", { exact: true }),
  ).toBeVisible();
  await expect
    .poll(() => page.evaluate(() => document.visibilityState))
    .toBe("visible");
  await expect(page.locator("[data-sonner-toast]")).toHaveCSS("opacity", "1");
  await page.screenshot({ path: test.info().outputPath("silent-warning.png") });
});

test("native close event stops an active test even without a visibility event", async ({
  page,
}) => {
  await page.goto("/tests/fixtures/audio.html");
  await startTest(page);
  await expect(page.getByRole("meter")).toBeVisible();
  await emit(page, "tauri://close-requested", null);
  await expectCommandCount(page, "stop_microphone_test", 1);
  await expect(page.getByRole("meter")).toHaveCount(0);
});

test("disconnect during pending start surfaces failure without reviving the meter", async ({
  page,
}) => {
  await page.goto("/tests/fixtures/audio.html?delayed");
  await startTest(page);
  await emit(page, "microphone-test-stopped-event", {
    session_id: 1,
    failed: true,
  });
  await page.evaluate(() => window.audioTest.resolveStart(1));
  await expect(page.getByRole("meter")).toHaveCount(0);
  await expect(
    page.getByText("Microphone test failed", { exact: true }),
  ).toBeVisible();
});
