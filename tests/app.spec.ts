import { test, expect } from "@playwright/test";

test.describe("Handy App", () => {
  test("dev server responds", async ({ page }) => {
    // Just verify the dev server is running and responds
    const response = await page.goto("/");
    expect(response?.status()).toBe(200);
  });

  test("page has html structure", async ({ page }) => {
    await page.goto("/");

    // Verify basic HTML structure exists
    const html = await page.content();
    expect(html).toContain("<html");
    expect(html).toContain("<body");
  });
});

test("post_processing_settings_show_promptv3_and_capglue_unavailable_state", async ({
  page,
}) => {
  await page.goto("/");

  await page.evaluate(async () => {
    (window as any).__TAURI_OS_PLUGIN_INTERNALS__ = {
      platform: "linux",
      os_type: "linux",
      family: "unix",
      eol: "\n",
      version: "test",
      arch: "x86_64",
      exe_extension: "",
    };

    const [React, ReactDom, settingsModule, storeModule, bindingsModule] =
      await Promise.all([
        import("/node_modules/.vite/deps/react.js"),
        import("/node_modules/.vite/deps/react-dom_client.js"),
        import(
          "/src/components/settings/post-processing/PostProcessingSettings.tsx"
        ),
        import("/src/stores/settingsStore.ts"),
        import("/src/bindings.ts"),
      ]);

    bindingsModule.commands.changePasteMethodSetting = async () => ({
      status: "ok",
      data: null,
    });

    const settings = {
      bindings: {
        transcribe_with_post_process: {
          id: "transcribe_with_post_process",
          name: "Transcribe with Post Process",
          description: "Transcribe and refine",
          default_binding: "CmdOrCtrl+Shift+P",
          current_binding: "CmdOrCtrl+Shift+P",
        },
      },
      push_to_talk: false,
      audio_feedback: false,
      paste_method: "ctrl_v",
      external_script_path: null,
      capglue_settings: { target: "", command: "capglue", args: [] },
      post_process_enabled: true,
      post_process_provider_id: "openai",
      post_process_providers: [
        { id: "openai", label: "OpenAI", base_url: "https://api.openai.com/v1" },
      ],
      post_process_api_keys: {},
      post_process_models: { openai: "gpt-4o-mini" },
      post_process_prompts: [
        {
          id: "promptv3",
          name: "promptv3",
          prompt: "Turn ${output} into a ready-to-use prompt.",
        },
      ],
      post_process_selected_prompt_id: "promptv3",
      experimental_enabled: true,
      keyboard_implementation: "tauri",
    };

    storeModule.useSettingsStore.setState({
      settings,
      defaultSettings: settings,
      isLoading: false,
      isUpdating: {},
      postProcessModelOptions: {},
      settingErrors: {},
    });

    document.body.innerHTML = '<div id="test-root"></div>';
    const createRoot = ReactDom.createRoot ?? ReactDom.default.createRoot;
    const createElement = React.createElement ?? React.default.createElement;
    createRoot(document.getElementById("test-root")).render(
      createElement(settingsModule.PostProcessingSettings),
    );
  });

  await expect(page.getByText("promptv3").first()).toBeVisible();
  await expect(page.getByLabel("Prompt label")).toHaveValue("promptv3");
  await expect(page.getByLabel("Prompt instructions")).toHaveValue(
    "Turn ${output} into a ready-to-use prompt.",
  );

  await page.getByRole("button", { name: /Clipboard/ }).click();
  await page.getByRole("button", { name: "Capglue" }).click();
  await expect(
    page.getByText(/Capglue is selected but no target is configured yet/),
  ).toBeVisible();
  await expect(page.getByPlaceholder("Capglue target (required)")).toBeVisible();
});

test("capglue_invalid_save_rolls_back_and_exposes_error", async ({ page }) => {
  await page.goto("/");

  const result = await page.evaluate(async () => {
    const [{ commands }, { useSettingsStore }] = await Promise.all([
      import("/src/bindings.ts"),
      import("/src/stores/settingsStore.ts"),
    ]);

    const persistedSettings = {
      capglue_settings: {
        target: "com.persisted.Target",
        command: "capglue",
        args: [],
      },
    };

    commands.getAppSettings = async () => ({
      status: "ok",
      data: persistedSettings,
    });
    commands.changeCapglueSettingsSetting = async () => ({
      status: "error",
      error: "capglue target is required",
    });

    useSettingsStore.setState({
      settings: persistedSettings,
      defaultSettings: persistedSettings,
      isLoading: false,
      isUpdating: {},
    });

    await useSettingsStore.getState().updateSetting("capglue_settings", {
      target: "",
      command: "capglue",
      args: [],
    });

    return {
      capglueSettings: useSettingsStore.getState().settings?.capglue_settings,
      error: useSettingsStore.getState().getSettingError("capglue_settings"),
    };
  });

  expect(result.capglueSettings).toEqual({
    target: "com.persisted.Target",
    command: "capglue",
    args: [],
  });
  expect(result.error).toContain("capglue target is required");
});
