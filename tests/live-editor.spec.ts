import { test, expect, type Page } from "@playwright/test";
import type {} from "./fixtures/live-editor";

// Actual overlay component and translations; mocked audio/ASR/Tauri/native paste.
// Browser textarea focus is covered. Native target capture/restoration is not.
const fixture = `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>Live editor mocked IPC fixture</title></head>
<body><main id="root"></main><script type="module">
import RefreshRuntime from '/@react-refresh';
RefreshRuntime.injectIntoGlobalHook(window);
window.$RefreshReg$ = () => {};
window.$RefreshSig$ = () => (type) => type;
window.__vite_plugin_react_preamble_installed__ = true;
await import('/tests/fixtures/live-editor.tsx');
</script></body></html>`;

const openEditor = async (page: Page) => {
  await page.setViewportSize({ width: 400, height: 320 });
  await page.route("**/__live-editor-test", (route) =>
    route.fulfill({ contentType: "text/html", body: fixture }),
  );
  await page.goto("/__live-editor-test");
  await page
    .getByRole("button", { name: "Edit live transcript", exact: true })
    .click();
  await expect(
    page.getByRole("textbox", { name: "Edit transcript", exact: true }),
  ).toBeEditable();
};

test.describe("Live editor component (mocked recognition and native IPC)", () => {
  test.beforeEach(async ({ page }) => openEditor(page));

  test("keeps recording while user edits and new speech appends without moving the caret", async ({
    page,
  }, testInfo) => {
    const editor = page.getByRole("textbox", {
      name: "Edit transcript",
      exact: true,
    });
    await editor.fill("Hello Alan");
    await expect
      .poll(() => page.evaluate(() => window.editorFixture.snapshot.text))
      .toBe("Hello Alan");
    await editor.evaluate((element: HTMLTextAreaElement) =>
      element.setSelectionRange(6, 10),
    );
    await page.evaluate(() =>
      window.editorFixture.appendRecognition("Hello world and welcome"),
    );
    await expect(editor).toHaveValue("Hello Alan and welcome");
    await expect
      .poll(() =>
        editor.evaluate((element: HTMLTextAreaElement) => [
          element.selectionStart,
          element.selectionEnd,
        ]),
      )
      .toEqual([6, 10]);
    await expect(
      page.getByText("Recording continues while you edit", { exact: true }),
    ).toBeVisible();
    expect(await page.evaluate(() => window.editorFixture.snapshot.phase)).toBe(
      "recording",
    );
    await page.evaluate(() =>
      window.editorFixture.emitStream(
        "Direct stream must not replace the draft",
      ),
    );
    await expect(editor).toHaveValue("Hello Alan and welcome");
    await page.screenshot({
      path: testInfo.outputPath("live-editor-recording-mocked.png"),
      fullPage: true,
    });
  });

  test("Stop & review waits for final recognition and a second explicit Paste", async ({
    page,
  }) => {
    const editor = page.getByRole("textbox", {
      name: "Edit transcript",
      exact: true,
    });
    await editor.fill("Hello Alan");
    await editor.press("Control+Enter");
    await expect(
      page.getByText("Finalizing transcript...", { exact: true }),
    ).toBeVisible();
    await expect(editor).not.toBeEditable();
    expect(await page.evaluate(() => window.editorFixture.pasted)).toEqual([]);
    await page.evaluate(() =>
      window.editorFixture.finishRecognition("Hello world."),
    );
    await expect(editor).toHaveValue("Hello Alan.");
    await expect(editor).toBeEditable();
    expect(await page.evaluate(() => window.editorFixture.pasted)).toEqual([]);
    await page.getByRole("button", { name: "Paste", exact: true }).click();
    await expect
      .poll(() => page.evaluate(() => window.editorFixture.pasted))
      .toEqual(["Hello Alan."]);
    await expect(editor).toHaveCount(0);
  });

  test("older acknowledgements never clobber pending typing or its ASR baseline", async ({
    page,
  }) => {
    const editor = page.getByRole("textbox", {
      name: "Edit transcript",
      exact: true,
    });
    await page.evaluate(() => {
      window.editorFixture.holdDrafts = true;
    });
    await editor.fill("My pending correction");
    await page.evaluate(() =>
      window.editorFixture.emitSnapshot({
        text: "Hello world new speech",
        asr_text: "Hello world new speech",
        user_revision: 0,
      }),
    );
    await expect(editor).toHaveValue("My pending correction");
    await editor.fill("My pending correction again");
    await expect
      .poll(() =>
        page.evaluate(() => window.editorFixture.drafts.at(-1)?.baseAsr),
      )
      .toBe("Hello world");
    await page.evaluate(() => window.editorFixture.flushDrafts());
    await expect(editor).toHaveValue("My pending correction again new speech");
  });

  test("target paste failure retains the draft and permits explicit retry", async ({
    page,
  }, testInfo) => {
    const editor = page.getByRole("textbox", {
      name: "Edit transcript",
      exact: true,
    });
    await editor.fill("Keep my corrected draft");
    await page.evaluate(() => window.editorFixture.finishRecognition());
    await page.evaluate(() => {
      window.editorFixture.failNextPaste = true;
    });
    await page.getByRole("button", { name: "Paste", exact: true }).click();
    await expect(page.getByRole("alert")).toContainText(
      "Check the destination before retrying. Your draft is kept.",
    );
    await expect(editor).toHaveValue("Keep my corrected draft");
    await expect(editor).toBeEditable();
    expect(await page.evaluate(() => window.editorFixture.pasted)).toEqual([]);
    await page.screenshot({
      path: testInfo.outputPath("live-editor-paste-failure-mocked.png"),
      fullPage: true,
    });
    await editor.press("Control+Enter");
    await expect
      .poll(() => page.evaluate(() => window.editorFixture.pasted))
      .toEqual(["Keep my corrected draft"]);
  });

  test("conflicts expose latest recognition beside the preserved editable draft", async ({
    page,
  }, testInfo) => {
    const editor = page.getByRole("textbox", {
      name: "Edit transcript",
      exact: true,
    });
    await editor.fill("My intended wording");
    await page.evaluate(() =>
      window.editorFixture.emitSnapshot({
        phase: "review",
        conflict: true,
        asr_text: "A substantially different final recognition",
        text: "My intended wording",
      }),
    );
    await expect(editor).toHaveValue("My intended wording");
    await expect(editor).toBeEditable();
    const recognition = page.getByRole("textbox", {
      name: "Latest recognition",
      exact: true,
    });
    await expect(recognition).toHaveValue(
      "A substantially different final recognition",
    );
    await expect(recognition).not.toBeEditable();
    await expect(page.getByRole("status")).toContainText(
      "Some new speech could not be merged",
    );
    await page.screenshot({
      path: testInfo.outputPath("live-editor-conflict-mocked.png"),
      fullPage: true,
    });
    await editor.press("Escape");
    await expect(editor).toHaveCount(0);
    expect(await page.evaluate(() => window.editorFixture.pasted)).toEqual([]);
  });
});
