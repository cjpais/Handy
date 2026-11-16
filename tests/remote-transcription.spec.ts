/**
 * UI e2e for Settings → Advanced → Remote Transcription.
 *
 * Runs the real React app in Chromium against a mocked Tauri IPC
 * (`window.__TAURI_INTERNALS__`). Every `invoke` is recorded so the test can
 * assert which backend command each interaction sends, and the remote
 * commands update the mocked settings the way the backend does.
 *
 * Run: npx playwright test tests/remote-transcription.spec.ts
 */
import { test, expect, type Page } from "@playwright/test";
import fs from "node:fs";

const SHOTS = process.env.SHOTS_DIR ?? "test-results/remote-transcription";

type Call = { cmd: string; args: Record<string, unknown> };
type Mock = {
  settings?: Record<string, unknown>;
  keychainLocked?: boolean;
  testResult?: { ok: true; ms: number } | { ok: false; error: string };
};

async function bootApp(page: Page, mock: Mock = {}) {
  await page.addInitScript((mock: Mock) => {
    type Server = Record<string, unknown> & { id: string; url: string };
    const settings: Record<string, unknown> = {
      onboarding_completed: true,
      selected_model: "",
      theme: "system",
      app_language: "en",
      experimental_enabled: false,
      post_process_enabled: false,
      debug_mode: false,
      bindings: {},
      custom_words: [],
      post_process_providers: [],
      post_process_api_keys: {},
      post_process_models: {},
      post_process_prompts: [],
      spoken_languages: ["th"],
      selected_language: "th",
      remote_transcription_enabled: false,
      remote_transcription_servers: [],
      remote_transcription_server_id: null,
      ...mock.settings,
    };
    const servers = () => settings.remote_transcription_servers as Server[];
    const calls: Call[] = [];
    (window as unknown as { __calls: Call[] }).__calls = calls;
    let nextId = 1;

    // What the backend commands do, minus persistence.
    const reply = (cmd: string, args: Record<string, unknown>): unknown => {
      switch (cmd) {
        case "get_app_settings":
        case "get_default_settings":
          return structuredClone(settings);
        case "change_remote_transcription_enabled_setting":
          settings.remote_transcription_enabled = args.enabled;
          return null;
        case "save_remote_transcription_server": {
          const server = structuredClone(args.server) as Server;
          server.url = server.url.trim();
          if (server.url && !/^https?:\/\//.test(server.url))
            throw `urlScheme|${server.url}`;
          if (server.body === "json") {
            const body = String(server.json_body ?? "");
            if (!body.includes("{audio_base64}")) throw "jsonNeedsAudio|";
            try {
              JSON.parse(body.replace(/\{[a-z_]+(\|[^}]*)?\}/g, "x"));
            } catch {
              throw "jsonInvalid|expected value at line 1";
            }
          }
          const i = servers().findIndex((s) => s.id === server.id);
          // Like the backend: a typed key goes to the keychain, and only the
          // fact that there is one is kept.
          server.has_api_key = server.api_key
            ? true
            : Boolean(servers()[i]?.has_api_key);
          server.api_key = "";
          if (i >= 0) servers()[i] = server;
          // Like the backend: saving never puts a server in use.
          else servers().push(server);
          return null;
        }
        case "clear_remote_transcription_api_key":
          if (mock.keychainLocked) throw "keychain|The keychain is locked";
          for (const server of servers())
            if (server.id === args.id) server.has_api_key = false;
          return null;
        case "select_remote_transcription_server":
          settings.remote_transcription_server_id = args.id;
          return null;
        case "delete_remote_transcription_server":
          settings.remote_transcription_servers = servers().filter(
            (s) => s.id !== args.id,
          );
          // Like the backend: never falls through to another server.
          if (settings.remote_transcription_server_id === args.id)
            settings.remote_transcription_server_id = null;
          return null;
        case "test_remote_transcription": {
          const r = mock.testResult ?? { ok: true, ms: 420 };
          if (!r.ok) throw r.error;
          return { elapsed_ms: r.ms, text: "" };
        }
      }
      if (cmd.startsWith("plugin:event|listen")) return nextId++;
      if (cmd.startsWith("plugin:macos-permissions|check_")) return true;
      if (cmd.startsWith("plugin:")) return null;
      if (/^get_.*(models|devices|languages|notices|entries|list)/.test(cmd))
        return [];
      if (/^(get|list)_/.test(cmd) && /s$/.test(cmd)) return [];
      return null;
    };

    const w = window as unknown as Record<string, unknown>;
    w.__TAURI_OS_PLUGIN_INTERNALS__ = {
      eol: "\n",
      platform: "macos",
      version: "15.0",
      family: "unix",
      os_type: "macos",
      arch: "aarch64",
      exe_extension: "",
    };
    w.__TAURI_INTERNALS__ = {
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { windowLabel: "main", label: "main" },
      },
      transformCallback: (cb: unknown) => {
        const id = nextId++;
        w[`_${id}`] = cb;
        return id;
      },
      unregisterCallback: () => {},
      convertFileSrc: (p: string) => p,
      invoke: async (cmd: string, args: Record<string, unknown> = {}) => {
        calls.push({ cmd, args: structuredClone(args ?? {}) });
        return reply(cmd, args ?? {});
      },
    };
    w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
  }, mock);

  await page.goto("/");
  await page.getByText("Advanced", { exact: true }).first().click();
  await expect(page.getByText("Remote Transcription").first()).toBeVisible();
}

const remoteCalls = async (page: Page) =>
  (
    await page.evaluate(
      () => (window as unknown as { __calls: Call[] }).__calls,
    )
  ).filter((c) => /remote_transcription/.test(c.cmd));

const savedServers = async (page: Page) =>
  (await remoteCalls(page))
    .filter((c) => c.cmd === "save_remote_transcription_server")
    .map((c) => c.args.server as Record<string, unknown>);

const toggle = (page: Page) =>
  page.getByRole("checkbox", { name: "Use a Remote Server" });
const field = (page: Page, label: string) =>
  page.getByRole("textbox", { name: label, exact: true });

async function addServer(page: Page, preset: string) {
  await page.getByRole("button", { name: "Add server" }).click();
  await page.getByRole("menuitem", { name: new RegExp(`^${preset}`) }).click();
}

async function shot(page: Page, name: string, anchorHeading: string) {
  fs.mkdirSync(SHOTS, { recursive: true });
  await page
    .getByRole("heading", { name: anchorHeading, exact: true })
    .first()
    .scrollIntoViewIfNeeded();
  await page.screenshot({ path: `${SHOTS}/${name}.png` });
}

const OPENTHAI = {
  id: "ot",
  name: "OpenThai2.0 ASR",
  url: "http://gpu.lan:18081/v1/transcribe",
  api_key: "",
  headers: [{ name: "Authorization", value: "Bearer {api_key}" }],
  fields: [
    { name: "lang", value: "th-mix" },
    { name: "mode", value: "standard" },
  ],
  file_field: "file",
  text_paths: "display_text, text",
  timeout_secs: 120,
};
const WITH_OPENTHAI = {
  remote_transcription_enabled: true,
  remote_transcription_servers: [OPENTHAI],
  remote_transcription_server_id: "ot",
};

test.use({ viewport: { width: 720, height: 600 } });

test("empty state: add a server from a preset", async ({ page }) => {
  await bootApp(page);
  await expect(toggle(page)).not.toBeChecked();
  await expect(field(page, "Endpoint URL")).toHaveCount(0);
  await shot(page, "1-empty", "Use a Remote Server");

  await addServer(page, "OpenThai2.0 ASR");
  await expect(field(page, "Endpoint URL")).toHaveValue(
    "http://localhost:18081/v1/transcribe",
  );
  const [saved] = await savedServers(page);
  expect(saved).toMatchObject({
    name: "OpenThai2.0 ASR",
    url: "http://localhost:18081/v1/transcribe",
    fields: [
      { name: "lang", value: "th-mix" },
      { name: "mode", value: "standard" },
    ],
    text_paths: "display_text, text",
  });
  // Adding a server never puts it in use, not even the first one.
  const commandsSent = async () => (await remoteCalls(page)).map((c) => c.cmd);
  expect(await commandsSent()).not.toContain(
    "select_remote_transcription_server",
  );
  const notInUse = page.getByText("Dictation isn't using this server.");
  await expect(notInUse).toBeVisible();
  await page.getByRole("button", { name: "Use This Server" }).click();
  await expect(notInUse).toHaveCount(0);
  expect(await commandsSent()).toContain("select_remote_transcription_server");
  await expect(field(page, "Name")).toHaveValue("OpenThai2.0 ASR");

  await toggle(page).click({ force: true });
  await expect(toggle(page)).toBeChecked();
  // A URL is set, so no warning.
  await expect(page.getByText("No server is in use yet")).toHaveCount(0);
});

test("edits save on blur, once, and bad URLs are explained", async ({
  page,
}) => {
  await bootApp(page, { settings: WITH_OPENTHAI });
  const url = field(page, "Endpoint URL");
  await url.fill("  http://10.0.0.5:18081/v1/transcribe  ");
  expect(await savedServers(page)).toEqual([]);
  await url.blur();
  await expect.poll(() => savedServers(page)).toHaveLength(1);
  await expect(url).toHaveValue("http://10.0.0.5:18081/v1/transcribe");

  // Leaving without a change saves nothing.
  await url.focus();
  await url.blur();
  await page.waitForTimeout(200);
  expect(await savedServers(page)).toHaveLength(1);

  await url.fill("10.0.0.5:18081");
  await url.blur();
  await expect(
    page.getByText("must start with http:// or https://"),
  ).toBeVisible();

  // Request fields: change a value, add a row, remove a row.
  const values = page.getByRole("textbox", {
    name: "Field value",
    exact: true,
  });
  await values.first().fill("th-TH");
  await values.first().blur();
  await expect
    .poll(async () => (await savedServers(page)).at(-1)?.fields)
    .toEqual([
      { name: "lang", value: "th-TH" },
      { name: "mode", value: "standard" },
    ]);
  await page.getByRole("button", { name: "Add field" }).click();
  await expect(values).toHaveCount(3);
  // The empty row survives the save and takes a name.
  const names = page.getByRole("textbox", { name: "Field name", exact: true });
  await names.nth(2).fill("split");
  await names.nth(2).blur();
  await expect(names.nth(2)).toHaveValue("split");
  await expect(values).toHaveCount(3);
  await page.getByRole("button", { name: "Remove mode" }).click();
  await expect
    .poll(async () => (await savedServers(page)).at(-1)?.fields)
    .toEqual([
      { name: "lang", value: "th-TH" },
      { name: "split", value: "" },
    ]);
});

test("Test Connection reports success, goes stale on edit", async ({
  page,
}) => {
  await bootApp(page, { settings: WITH_OPENTHAI });
  await page.getByRole("button", { name: "Test Connection" }).click();
  await expect(
    page.getByText("Connected. The server answered in 0.4 s."),
  ).toBeVisible();
  const call = (await remoteCalls(page)).find(
    (c) => c.cmd === "test_remote_transcription",
  );
  expect(call?.args.server).toMatchObject({ id: "ot", url: OPENTHAI.url });
  await shot(page, "2-connected", "API Key");

  await field(page, "Name").fill("GPU box");
  await field(page, "Name").blur();
  await expect(page.getByText("Connected.")).toHaveCount(0);
});

test("a failed test shows why", async ({ page }) => {
  await bootApp(page, {
    settings: WITH_OPENTHAI,
    testResult: {
      ok: false,
      error: "unreachable|gpu.lan:18081 (Connection refused (os error 61))",
    },
  });
  await page.getByRole("button", { name: "Test Connection" }).click();
  await expect(
    page.getByText(
      "Couldn't connect to gpu.lan:18081 (Connection refused (os error 61)).",
    ),
  ).toBeVisible();
  await shot(page, "3-error", "Request Fields");
});

const WHISPER = {
  ...OPENTHAI,
  id: "wh",
  name: "whisper.cpp server",
  url: "http://127.0.0.1:8080/inference",
};
const TWO_SERVERS = {
  ...WITH_OPENTHAI,
  remote_transcription_servers: [OPENTHAI, WHISPER],
};

test("looking at another server doesn't reroute dictation", async ({
  page,
}) => {
  await bootApp(page, { settings: TWO_SERVERS });
  const footer = page.locator(".border-t").last();
  const selects = async () =>
    (await remoteCalls(page)).filter(
      (c) => c.cmd === "select_remote_transcription_server",
    );
  const notInUse = page.getByText("Dictation isn't using this server.");
  await expect(footer.getByText("OpenThai2.0 ASR")).toBeVisible();

  // Open the other server: the editor follows, dictation doesn't.
  await page.getByRole("button", { name: "OpenThai2.0 ASR (in use)" }).click();
  await page.getByRole("button", { name: "whisper.cpp server" }).click();
  await expect(field(page, "Endpoint URL")).toHaveValue(WHISPER.url);
  await expect(notInUse).toBeVisible();
  await expect(footer.getByText("OpenThai2.0 ASR")).toBeVisible();
  expect(await selects()).toEqual([]);
  await shot(page, "10-not-in-use", "Server");

  // Adding one is the same: it opens, nothing is rerouted.
  await addServer(page, "MLX Audio");
  await expect(field(page, "Name")).toHaveValue("MLX Audio");
  await expect(footer.getByText("OpenThai2.0 ASR")).toBeVisible();
  expect(await selects()).toEqual([]);

  // Only the button switches.
  await page.getByRole("button", { name: "Use This Server" }).click();
  await expect(footer.getByText("MLX Audio")).toBeVisible();
  expect(await selects()).toHaveLength(1);
  await expect(notInUse).toHaveCount(0);
});

test("deleting takes two clicks and never picks another server", async ({
  page,
}) => {
  await bootApp(page, { settings: TWO_SERVERS });
  const footer = page.locator(".border-t").last();
  // One click only arms the delete.
  await page.getByRole("button", { name: "Delete Server" }).click();
  await expect(field(page, "Endpoint URL")).toHaveValue(OPENTHAI.url);
  await page.getByRole("button", { name: "Click again to delete" }).click();

  // The server in use is gone: the other one is shown, but not used.
  await expect(field(page, "Endpoint URL")).toHaveValue(WHISPER.url);
  await expect(page.getByText("No server is in use yet")).toBeVisible();
  await expect(
    page.getByText("Dictation isn't using this server."),
  ).toBeVisible();
  await expect(footer.getByText("whisper.cpp server")).toHaveCount(0);
});

test("an emptied timeout goes back to the default", async ({ page }) => {
  await bootApp(page, { settings: WITH_OPENTHAI });
  await page.getByText("More Options", { exact: true }).click();
  const timeout = page.getByRole("spinbutton", { name: "Timeout (seconds)" });
  const saved = async () => (await savedServers(page)).at(-1)?.timeout_secs;
  await timeout.fill("30");
  await timeout.blur();
  await expect.poll(saved).toBe(30);
  await timeout.fill("");
  await timeout.blur();
  await expect.poll(saved).toBe(120);
});

test("enabled with no server warns", async ({ page }) => {
  await bootApp(page, { settings: { remote_transcription_enabled: true } });
  await expect(page.getByText("No server is in use yet")).toBeVisible();
  await expect(page.getByText("No servers yet")).toBeVisible();
  await page.getByRole("button", { name: "Add server" }).click();
  // Only servers people run themselves; anything else is a custom server.
  await expect(page.getByRole("menuitem")).toHaveCount(6);
  await expect(
    page.getByRole("menuitem", { name: /^Custom server/ }),
  ).toBeVisible();
  await shot(page, "4-needs-server-menu", "Use a Remote Server");
  await page.keyboard.press("Escape");
  await expect(page.getByRole("menu")).toHaveCount(0);
});

test("dark mode, advanced section open", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "dark" });
  await bootApp(page, { settings: WITH_OPENTHAI });
  await shot(page, "5-dark", "API Key");
  await page.getByText("More Options", { exact: true }).click();
  await expect(field(page, "Text in Response")).toHaveValue(
    "display_text, text",
  );
  await shot(page, "6-dark-advanced", "Headers");
});

test("audio LLM preset: a plain prompt, no JSON to write", async ({ page }) => {
  await bootApp(page, { settings: { remote_transcription_enabled: true } });
  await addServer(page, "llama.cpp audio model");
  const prompt = page.getByRole("textbox", { name: "Prompt" });
  await expect(prompt).toHaveValue(/^You are a speech-to-text engine\./);
  await expect(page.getByRole("textbox", { name: "JSON Body" })).toHaveCount(0);
  await expect(page.getByText("Request Fields")).toBeVisible();
  const [saved] = await savedServers(page);
  expect(saved).toMatchObject({
    body: "chat",
    text_paths: "choices.0.message.content",
    fields: [
      { name: "model", value: "" },
      { name: "temperature", value: "0" },
    ],
  });
  await shot(page, "7-chat", "Send Audio As");

  await prompt.fill("ถอดเสียงเป็นข้อความภาษาไทย");
  await prompt.blur();
  await expect
    .poll(async () => (await savedServers(page)).at(-1)?.prompt)
    .toBe("ถอดเสียงเป็นข้อความภาษาไทย");
});

test("send mode can be switched; a refused save keeps what was typed", async ({
  page,
}) => {
  await bootApp(page, { settings: WITH_OPENTHAI });
  const textPath = async () => (await savedServers(page)).at(-1)?.text_paths;

  // Form upload -> Custom JSON: a body to start from, and the chat text path.
  await page.getByRole("button", { name: "Form upload (multipart)" }).click();
  await page.getByRole("button", { name: "Custom JSON" }).click();
  const jsonBody = page.getByRole("textbox", { name: "JSON Body" });
  await expect(jsonBody).toHaveValue(/"data": "\{audio_base64\}"/);
  await expect(page.getByText("Request Fields")).toHaveCount(0);
  // OpenThai's own path isn't the default, so it is left alone.
  expect(await textPath()).toBe("display_text, text");
  await shot(page, "9-custom-json", "Send Audio As");

  // Broken JSON: explained, and still there to fix.
  const broken = '{"audio": "{audio_base64}",}';
  await jsonBody.fill(broken);
  await jsonBody.blur();
  await expect(
    page.getByText("The JSON body isn't valid JSON: expected value at line 1"),
  ).toBeVisible();
  await expect(jsonBody).toHaveValue(broken);

  // Custom JSON -> Chat -> Form upload.
  await jsonBody.fill('{"audio": "{audio_base64}"}');
  await jsonBody.blur();
  await page.getByRole("button", { name: "Custom JSON" }).click();
  await page.getByRole("button", { name: "Chat with audio (LLM)" }).click();
  await expect(page.getByRole("textbox", { name: "Prompt" })).toHaveValue(
    /speech-to-text engine/,
  );
  await page.getByRole("button", { name: "Chat with audio (LLM)" }).click();
  await page.getByRole("button", { name: "Form upload (multipart)" }).click();
  await expect(page.getByText("Request Fields")).toBeVisible();
  await expect(jsonBody).toHaveCount(0);
});

test("the text path follows the send mode when it was the default", async ({
  page,
}) => {
  await bootApp(page, { settings: { remote_transcription_enabled: true } });
  await addServer(page, "whisper.cpp server");
  await page.getByRole("button", { name: "Form upload (multipart)" }).click();
  await page.getByRole("button", { name: "Chat with audio (LLM)" }).click();
  await expect
    .poll(async () => (await savedServers(page)).at(-1)?.text_paths)
    .toBe("choices.0.message.content");
  await page.getByRole("button", { name: "Chat with audio (LLM)" }).click();
  await page.getByRole("button", { name: "Form upload (multipart)" }).click();
  await expect
    .poll(async () => (await savedServers(page)).at(-1)?.text_paths)
    .toBe("text");
});

test("a raw-body server says what is sent", async ({ page }) => {
  await bootApp(page, { settings: { remote_transcription_enabled: true } });
  await addServer(page, "Custom server");
  await page.getByRole("button", { name: "Form upload (multipart)" }).click();
  await page.getByRole("button", { name: "Raw audio body" }).click();
  await expect(
    page.getByText("The WAV is the whole request body"),
  ).toBeVisible();
  await expect(page.getByText("Request Fields")).toHaveCount(0);
  await expect
    .poll(async () => (await savedServers(page)).at(-1)?.body)
    .toBe("raw");
  await shot(page, "8-raw-body", "Send Audio As");
});

test("an API key is entered, never shown again, and can be removed", async ({
  page,
}) => {
  await bootApp(page, { settings: WITH_OPENTHAI });
  const key = field(page, "API Key");
  await expect(key).toHaveAttribute("type", "password");
  await expect(key).toHaveAttribute("placeholder", "Optional");

  await key.fill("sk-secret");
  await key.blur();
  await expect
    .poll(async () => (await savedServers(page)).at(-1)?.api_key)
    .toBe("sk-secret");
  // The window gets no key back: only that one is saved.
  await expect(key).toHaveValue("");
  await expect(key).toHaveAttribute(
    "placeholder",
    "Saved in your keychain. Type to replace it",
  );
  const stored = await page.evaluate(() =>
    (
      window as unknown as {
        __TAURI_INTERNALS__: {
          invoke: (c: string) => Promise<Record<string, unknown>>;
        };
      }
    ).__TAURI_INTERNALS__.invoke("get_app_settings"),
  );
  expect(JSON.stringify(stored)).not.toContain("sk-secret");

  // Another edit keeps the key without resending it.
  await field(page, "Name").fill("GPU box");
  await field(page, "Name").blur();
  await expect
    .poll(async () => (await savedServers(page)).at(-1)?.name)
    .toBe("GPU box");
  expect((await savedServers(page)).at(-1)?.api_key).toBe("");
  await expect(key).toHaveAttribute(
    "placeholder",
    "Saved in your keychain. Type to replace it",
  );

  await page.getByRole("button", { name: "Remove", exact: true }).click();
  await expect(key).toHaveAttribute("placeholder", "Optional");
  expect((await remoteCalls(page)).map((c) => c.cmd)).toContain(
    "clear_remote_transcription_api_key",
  );
});

test("plain http to a public host is called out; local and https are not", async ({
  page,
}) => {
  await bootApp(page, { settings: WITH_OPENTHAI });
  const warning = page.getByText("This URL isn't encrypted (http://)");
  const url = field(page, "Endpoint URL");
  // gpu.lan is on the local network.
  await expect(warning).toHaveCount(0);
  for (const local of [
    "http://localhost:8000/v1/audio/transcriptions",
    "http://192.168.1.10:18081/v1/transcribe",
    "http://100.101.102.103:8000/x",
    "https://asr.example.com/v1/audio/transcriptions",
  ]) {
    await url.fill(local);
    await url.blur();
    await expect(url).toHaveValue(local);
    await expect(warning).toHaveCount(0);
  }
  await url.fill("http://asr.example.com/v1/audio/transcriptions");
  await url.blur();
  await expect(warning).toBeVisible();
  await shot(page, "11-unencrypted", "Request Fields");
});

test("a key typed over a saved one is sent once, then cleared", async ({
  page,
}) => {
  await bootApp(page, {
    settings: {
      ...WITH_OPENTHAI,
      remote_transcription_servers: [{ ...OPENTHAI, has_api_key: true }],
    },
  });
  const key = field(page, "API Key");
  await key.fill("sk-new");
  await key.blur();
  await expect
    .poll(async () => (await savedServers(page)).at(-1)?.api_key)
    .toBe("sk-new");
  // Stored is unchanged (it already had a key), but the form lets it go.
  await expect(key).toHaveValue("");
  await field(page, "Name").fill("Renamed");
  await field(page, "Name").blur();
  await expect
    .poll(async () => (await savedServers(page)).at(-1)?.name)
    .toBe("Renamed");
  expect((await savedServers(page)).at(-1)?.api_key).toBe("");
});

test("quick edits in a row all survive", async ({ page }) => {
  await bootApp(page, { settings: WITH_OPENTHAI });
  const name = field(page, "Name");
  const url = field(page, "Endpoint URL");
  // No wait between them: each save's refresh lands after the next edit.
  await name.fill("A");
  await url.focus();
  await url.fill("http://10.0.0.9:18081/v1/transcribe");
  await name.focus();
  await name.fill("B");
  await url.focus();
  await expect
    .poll(async () => {
      const last = (await savedServers(page)).at(-1);
      return [last?.name, last?.url];
    })
    .toEqual(["B", "http://10.0.0.9:18081/v1/transcribe"]);
  await expect(name).toHaveValue("B");
  await expect(url).toHaveValue("http://10.0.0.9:18081/v1/transcribe");
});

test("the http warning looks at real addresses, not at names", async ({
  page,
}) => {
  await bootApp(page, { settings: WITH_OPENTHAI });
  const warning = page.getByText("This URL isn't encrypted (http://)");
  const url = field(page, "Endpoint URL");
  for (const [value, warns] of [
    ["http://fd-asr.example.com/v1/transcribe", true],
    ["http://10.example.com/v1/transcribe", true],
    ["http://[fd12:3456::1]:8000/v1/transcribe", false],
    ["http://gpu.home.arpa:8000/v1/transcribe", false],
  ] as const) {
    await url.fill(value);
    await url.blur();
    await expect(url).toHaveValue(value);
    await expect(warning).toHaveCount(warns ? 1 : 0);
  }
});

test("a placeholder error names the setting as the page does", async ({
  page,
}) => {
  await bootApp(page, {
    settings: WITH_OPENTHAI,
    testResult: { ok: false, error: "placeholder|Prompt" },
  });
  await page.getByRole("button", { name: "Test Connection" }).click();
  await expect(
    page.getByText(
      "The Prompt uses a placeholder that has no value here. Give it a fallback, such as {language|auto}.",
    ),
  ).toBeVisible();
});

test("testing right after typing a key still shows the result", async ({
  page,
}) => {
  await bootApp(page, { settings: WITH_OPENTHAI });
  const key = field(page, "API Key");
  await key.fill("sk-new");
  // Clicking Test blurs the field: the key is saved while the test runs.
  await page.getByRole("button", { name: "Test Connection" }).click();
  await expect(
    page.getByText("Connected. The server answered in 0.4 s."),
  ).toBeVisible();
  await expect(key).toHaveValue("");
});

test("a key removal that fails says why", async ({ page }) => {
  await bootApp(page, {
    settings: {
      ...WITH_OPENTHAI,
      remote_transcription_servers: [{ ...OPENTHAI, has_api_key: true }],
    },
    keychainLocked: true,
  });
  await page.getByRole("button", { name: "Remove", exact: true }).click();
  await expect(
    page.getByText("Couldn't use the system keychain: The keychain is locked"),
  ).toBeVisible();
  // Still saved: the form says so.
  await expect(field(page, "API Key")).toHaveAttribute(
    "placeholder",
    "Saved in your keychain. Type to replace it",
  );
});
