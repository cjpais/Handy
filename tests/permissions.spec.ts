import { expect, test, type Page } from "@playwright/test";

interface PermissionScenario {
  platform?: "linux" | "macos" | "windows";
  wayland?: boolean;
  authorized?: boolean;
  grantAuthorization?: boolean;
  pasteMethod?: string;
  firstRun?: boolean;
}

const pageErrors = new WeakMap<Page, string[]>();

test.beforeEach(({ page }) => {
  const errors: string[] = [];
  pageErrors.set(page, errors);
  page.on("pageerror", (error) => errors.push(error.message));
});

test.afterEach(({ page }) => {
  expect(pageErrors.get(page)).toEqual([]);
});

async function openApp(page: Page, scenario: PermissionScenario = {}) {
  await page.addInitScript((scenario) => {
    const platform = scenario.platform ?? "linux";
    const settings = {
      onboarding_completed: !scenario.firstRun,
      app_language: "en",
      spoken_languages: ["en"],
      selected_language: "auto",
      selected_model: "",
      bindings: {},
      paste_method: scenario.pasteMethod ?? "direct",
      typing_tool: "auto",
      remote_desktop_key_event_delay_ms: 1,
      show_whats_new_on_update: false,
      theme: "light",
    };
    let authorized = scenario.authorized ?? false;
    let microphone = false;
    let accessibility = false;
    let callbackId = 0;
    const callbacks = new Map<number, (value: unknown) => void>();
    const authorizationActions: string[] = [];
    if (scenario.firstRun) {
      localStorage.setItem(
        "handy.onboarding",
        JSON.stringify({ step: "permissions" }),
      );
    }

    Object.assign(window, {
      __testAuthorizationActions: authorizationActions,
      __TAURI_OS_PLUGIN_INTERNALS__: {
        platform,
        os_type: platform,
        family: platform === "windows" ? "windows" : "unix",
        arch: "x86_64",
        eol: "\n",
      },
      __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener: () => {} },
      __TAURI_INTERNALS__: {
        metadata: {
          currentWindow: { label: "main" },
          currentWebview: { label: "main" },
        },
        transformCallback: (callback: (value: unknown) => void) => {
          callbacks.set(++callbackId, callback);
          return callbackId;
        },
        unregisterCallback: (id: number) => callbacks.delete(id),
        invoke: async (command: string, args: Record<string, unknown> = {}) => {
          switch (command) {
            case "get_app_settings":
            case "get_default_settings":
              return structuredClone(settings);
            case "is_wayland_active":
              return platform === "linux" && (scenario.wayland ?? true);
            case "get_remote_desktop_authorization":
              return authorized;
            case "request_remote_desktop_authorization":
              authorizationActions.push("request");
              authorized = scenario.grantAuthorization ?? true;
              return true;
            case "delete_remote_desktop_authorization":
              authorizationActions.push("revoke");
              authorized = false;
              return true;
            case "change_paste_method_setting":
              settings.paste_method = String(args.method);
              return;
            case "change_typing_tool_setting":
              settings.typing_tool = String(args.tool);
              return;
            case "get_available_typing_tools":
              return ["auto", "remote_desktop", "wtype"];
            case "get_windows_microphone_permission_status":
              return { supported: true, overall_access: "denied" };
            case "plugin:macos-permissions|check_microphone_permission":
              return microphone;
            case "plugin:macos-permissions|check_accessibility_permission":
              return accessibility;
            case "plugin:macos-permissions|request_microphone_permission":
              microphone = true;
              return;
            case "plugin:macos-permissions|request_accessibility_permission":
              accessibility = true;
              return;
            case "plugin:event|listen":
              return ++callbackId;
            case "plugin:app|version":
              return "0.10.0";
            case "plugin:os|locale":
              return "en-US";
            case "is_update_checks_locked":
              return true;
            case "get_current_model":
              return "";
            case "get_available_models":
            case "get_retired_models":
            case "get_available_microphones":
            case "get_available_output_devices":
            case "get_active_notices":
            case "get_transcription_jobs":
              return [];
            default:
              return null;
          }
        },
      },
    });
  }, scenario);
  await page.goto("/");
}

test("Wayland uses the shared permission title, action and granted state", async ({
  page,
}) => {
  await openApp(page);
  await expect(
    page.getByRole("heading", { name: "One permission, then you're set" }),
  ).toBeVisible();
  await expect(
    page.getByText("Typing authorization", { exact: true }),
  ).toHaveCount(1);
  await page.getByRole("button", { name: "Allow", exact: true }).click();
  await expect(page.getByText("Allowed", { exact: true })).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Continue", exact: true }),
  ).toBeEnabled();
});

test("a declined Wayland request can be retried or skipped", async ({
  page,
}) => {
  await openApp(page, { grantAuthorization: false });
  await page.getByRole("button", { name: "Allow", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "Allow", exact: true }),
  ).toBeEnabled();
  await expect(
    page.getByRole("button", { name: "Continue", exact: true }),
  ).toBeDisabled();
  await page.getByRole("button", { name: "Not now", exact: true }).click();
  await expect(page.getByText("Advanced", { exact: true })).toBeVisible();
  await page.getByText("Advanced", { exact: true }).click();
  await expect(
    page.getByRole("checkbox", { name: "Typing authorization", exact: true }),
  ).not.toBeChecked();
});

test("output changes expose the authorization control without reopening onboarding", async ({
  page,
}) => {
  await openApp(page, { pasteMethod: "none" });
  await page.getByText("Advanced", { exact: true }).click();
  await expect(
    page.getByText("Typing authorization", { exact: true }),
  ).toHaveCount(0);
  await page.getByRole("button", { name: "None", exact: true }).click();
  await page.getByRole("button", { name: "Direct", exact: true }).click();
  const authorization = page.getByRole("checkbox", {
    name: "Typing authorization",
    exact: true,
  });
  await expect(authorization).toBeVisible();
  await expect(authorization).not.toBeChecked();
  await expect(
    page.getByRole("heading", { name: "One permission, then you're set" }),
  ).toHaveCount(0);
  await page
    .getByRole("button", { name: "Auto (Recommended)", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Remote Desktop (Portal)", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "One permission, then you're set" }),
  ).toHaveCount(0);
  await expect(authorization).not.toBeChecked();
  await authorization.locator("..").click();
  await expect(authorization).toBeChecked();
  await expect(page.getByText("Configured", { exact: true })).toBeVisible();
});

test("an authorized Wayland user can revoke and request authorization in settings", async ({
  page,
}) => {
  await openApp(page, { authorized: true });
  await page.getByText("Advanced", { exact: true }).click();
  await expect(
    page.getByText("Typing authorization", { exact: true }),
  ).toHaveCount(1);
  const authorization = page.getByRole("checkbox", {
    name: "Typing authorization",
    exact: true,
  });
  await expect(authorization).toBeChecked();
  const output = page
    .getByRole("heading", { name: "Output", exact: true })
    .locator("../..");
  await expect(output.getByRole("spinbutton")).toHaveValue("1");
  await authorization.locator("..").click();
  await expect(authorization).not.toBeChecked();
  await expect(
    page.getByText("Configuration is required", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "One permission, then you're set" }),
  ).toHaveCount(0);
  await authorization.locator("..").click();
  await expect(authorization).toBeChecked();
  await expect(page.getByText("Configured", { exact: true })).toBeVisible();
  expect(
    await page.evaluate(() =>
      Reflect.get(window, "__testAuthorizationActions"),
    ),
  ).toEqual(["revoke", "request"]);
});

for (const pasteMethod of ["ctrl_v", "ctrl_shift_v", "shift_insert"]) {
  test(`Wayland shows authorization for ${pasteMethod} clipboard paste`, async ({
    page,
  }) => {
    await openApp(page, { authorized: true, pasteMethod });
    await page.getByText("Advanced", { exact: true }).click();
    const authorization = page.getByRole("checkbox", {
      name: "Typing authorization",
      exact: true,
    });
    await expect(authorization).toBeChecked();
    await authorization.locator("..").click();
    await expect(authorization).not.toBeChecked();
    expect(
      await page.evaluate(() =>
        Reflect.get(window, "__testAuthorizationActions"),
      ),
    ).toEqual(["revoke"]);
  });
}

test("first-run Wayland onboarding uses the same permission screen", async ({
  page,
}) => {
  await openApp(page, { firstRun: true });
  await expect(
    page.getByRole("heading", { name: "One permission, then you're set" }),
  ).toBeVisible();
  await expect(
    page.getByText("Typing authorization", { exact: true }),
  ).toHaveCount(1);
  await expect(
    page.getByRole("button", { name: "Allow", exact: true }),
  ).toBeEnabled();
  await expect(
    page.getByRole("button", { name: "Not now", exact: true }),
  ).toBeEnabled();
});

test("X11 does not request portal permissions when the output method changes", async ({
  page,
}) => {
  await openApp(page, { wayland: false, pasteMethod: "none" });
  await page.getByText("Advanced", { exact: true }).click();
  await page.getByRole("button", { name: "None", exact: true }).click();
  await page.getByRole("button", { name: "Direct", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "Auto (Recommended)", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText("Typing authorization", { exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("heading", { name: "Typing delay", exact: true }),
  ).toHaveCount(0);
});

test("macOS keeps its existing two-permission flow", async ({ page }) => {
  await openApp(page, { platform: "macos" });
  await expect(
    page.getByRole("heading", { name: "Two permissions, then you're set" }),
  ).toBeVisible();
  const allow = page.getByRole("button", { name: "Allow", exact: true });
  await expect(allow).toHaveCount(2);
  await expect(allow.nth(1)).toBeDisabled();
  await allow.nth(0).click();
  await expect(allow).toHaveCount(1);
  await allow.click();
  await expect(
    page.getByRole("button", { name: "Continue", exact: true }),
  ).toBeEnabled();
});

test("Windows keeps its microphone permission screen", async ({ page }) => {
  await openApp(page, { platform: "windows" });
  await expect(
    page.getByRole("heading", { name: "One permission, then you're set" }),
  ).toBeVisible();
  await expect(
    page.getByText("Windows asks before any app can listen.", { exact: true }),
  ).toBeVisible();
  await expect(page.getByText("Microphone", { exact: true })).toBeVisible();
  await expect(
    page.getByText("Typing authorization", { exact: true }),
  ).toHaveCount(0);
});
