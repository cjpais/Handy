// Standalone regression check: bun run test:post-process-models
import assert from "node:assert/strict";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { AppSettings, PostProcessApiFormat } from "@/bindings";
import { useSettingsStore } from "./settingsStore";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

const originalState = useSettingsStore.getState();
const originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
const originalConsoleError = console.error;
Object.defineProperty(globalThis, "window", { value: {}, configurable: true });
console.error = () => {};

const store = () => useSettingsStore.getState();
const modelOptions = (id = "custom") =>
  store().postProcessModelOptions[id] ?? [];
const isFetching = (id = "custom") =>
  store().isUpdatingKey(`post_process_models_fetch:${id}`);

function fixture() {
  useSettingsStore.setState(originalState, true);
  const settings: AppSettings = {
    post_process_provider_id: "custom",
    post_process_providers: [
      {
        id: "custom",
        label: "Custom",
        base_url: "https://old.invalid/v1",
        api_format: "openai",
      },
      {
        id: "openai",
        label: "Other",
        base_url: "https://other.invalid/v1",
        api_format: "openai",
      },
    ],
    post_process_api_keys: { custom: "test-key", openai: "test-other-key" },
    post_process_models: { custom: "old-model" },
  };
  store().setSettings(structuredClone(settings));
  const tasks: Promise<unknown>[] = [];
  const requests: Array<{
    providerId: string;
    baseUrl: string | undefined;
    response: ReturnType<typeof deferred<string[]>>;
  }> = [];
  let cleaningUp = false;
  const gates = new Map<string, ReturnType<typeof deferred<void>>>();
  const allGates: ReturnType<typeof deferred<void>>[] = [];
  const failures = new Map<string, unknown>();
  const track = <T>(task: Promise<T>) => {
    tasks.push(task);
    return task;
  };

  mockIPC(async (command, args) => {
    const payload = args as Record<string, unknown>;
    const providerId = payload?.providerId as string;
    const gate = gates.get(command);
    gates.delete(command);
    if (gate) await gate.promise;
    if (failures.has(command)) {
      const error = failures.get(command);
      failures.delete(command);
      throw error;
    }
    const provider = settings.post_process_providers?.find(
      (p) => p.id === providerId,
    );
    switch (command) {
      case "fetch_post_process_models": {
        const response = deferred<string[]>();
        requests.push({ providerId, baseUrl: provider?.base_url, response });
        if (cleaningUp) response.resolve([]);
        return response.promise;
      }
      case "change_post_process_base_url_setting":
        assert(provider);
        provider.base_url = payload.baseUrl as string;
        return null;
      case "change_post_process_api_format_setting":
        assert(provider);
        provider.api_format = payload.apiFormat as PostProcessApiFormat;
        return null;
      case "change_post_process_api_key_setting":
        settings.post_process_api_keys![providerId] = payload.apiKey as string;
        return null;
      case "change_post_process_model_setting":
        settings.post_process_models![providerId] = payload.model as string;
        return null;
      case "set_post_process_provider":
        settings.post_process_provider_id = providerId;
        return null;
      case "get_app_settings":
        return structuredClone(settings);
      default:
        throw new Error(`Unexpected command: ${command}`);
    }
  });
  return {
    settings,
    requests,
    track,
    fetch: (id = "custom") => track(store().fetchPostProcessModels(id)),
    delay(command: string) {
      const gate = deferred<void>();
      gates.set(command, gate);
      allGates.push(gate);
      return gate;
    },
    fail: (command: string, error: unknown) => failures.set(command, error),
    async cleanup() {
      cleaningUp = true;
      allGates.forEach((gate) => gate.resolve());
      requests.forEach((request) => request.response.resolve([]));
      await Promise.allSettled(tasks);
      clearMocks();
    },
  };
}

type Fixture = ReturnType<typeof fixture>;
const cases: Array<{ name: string; run: (f: Fixture) => Promise<void> }> = [];
const check = (name: string, run: (f: Fixture) => Promise<void>) =>
  cases.push({ name, run });
const connectionChanges = [
  {
    name: "base URL",
    command: "change_post_process_base_url_setting",
    run: () =>
      store().updatePostProcessBaseUrl("custom", "https://new.invalid/v1"),
  },
  {
    name: "API format",
    command: "change_post_process_api_format_setting",
    run: () => store().updatePostProcessApiFormat("custom", "anthropic"),
  },
  {
    name: "API key",
    command: "change_post_process_api_key_setting",
    run: () => store().updatePostProcessApiKey("custom", "test-new-key"),
  },
  {
    name: "generic base URL setter",
    command: "change_post_process_base_url_setting",
    run: () =>
      store().updatePostProcessSetting(
        "base_url",
        "custom",
        "https://new.invalid/v1",
      ),
  },
];

check(
  "a current successful response populates its provider's cache",
  async (f) => {
    const pending = f.fetch();
    f.requests[0].response.resolve(["current"]);
    assert.deepEqual(await pending, ["current"]);
    assert.deepEqual(modelOptions(), ["current"]);
    assert.equal(isFetching(), false);
  },
);

check(
  "an older success cannot overwrite a newer successful response",
  async (f) => {
    const older = f.fetch();
    const newer = f.fetch();
    f.requests[1].response.resolve(["new"]);
    await newer;
    f.requests[0].response.resolve(["old"]);
    assert.deepEqual(await older, []);
    assert.deepEqual(modelOptions(), ["new"]);
  },
);

for (const outcome of ["success", "error"] as const) {
  check(
    `an older ${outcome} cannot clear a newer request's loading flag`,
    async (f) => {
      const older = f.fetch();
      const newer = f.fetch();
      if (outcome === "success") f.requests[0].response.resolve(["old"]);
      else f.requests[0].response.reject("old request failed");
      await older;
      assert.equal(isFetching(), true);
      f.requests[1].response.resolve(["new"]);
      await newer;
      assert.equal(isFetching(), false);
    },
  );
}

for (const change of connectionChanges) {
  check(`${change.name} invalidates an in-flight model list`, async (f) => {
    const older = f.fetch();
    await f.track(change.run());
    f.requests[0].response.resolve(["old"]);
    assert.deepEqual(await older, []);
    assert.deepEqual(modelOptions(), []);
    assert.equal(isFetching(), false);
  });
  check(
    `a refresh during ${change.name} waits for the save and runs afterward`,
    async (f) => {
      const gate = f.delay(change.command);
      const saving = f.track(change.run());
      const queued = f.fetch();
      assert.equal(f.requests.length, 0);
      assert.equal(isFetching(), true);
      gate.resolve();
      await saving;
      f.requests[0].response.resolve(["fresh"]);
      assert.deepEqual(await queued, ["fresh"]);
    },
  );
}

check(
  "a queued refresh waits for overlapping saves, including an edit started after the click",
  async (f) => {
    const format = f.delay("change_post_process_api_format_setting");
    const key = f.delay("change_post_process_api_key_setting");
    const savingFormat = f.track(
      store().updatePostProcessApiFormat("custom", "anthropic"),
    );
    const queued = f.fetch();
    const savingKey = f.track(
      store().updatePostProcessApiKey("custom", "test-new-key"),
    );
    format.resolve();
    await savingFormat;
    assert.equal(f.requests.length, 0);
    assert.equal(isFetching(), true);
    key.resolve();
    await savingKey;
    f.requests[0].response.resolve(["fresh"]);
    assert.deepEqual(await queued, ["fresh"]);
  },
);

check(
  "only the latest of two queued refreshes runs after a save",
  async (f) => {
    const gate = f.delay("change_post_process_base_url_setting");
    const saving = f.track(
      store().updatePostProcessBaseUrl("custom", "https://new.invalid/v1"),
    );
    const older = f.fetch();
    const newer = f.fetch();
    assert.equal(f.requests.length, 0);
    gate.resolve();
    await saving;
    assert.deepEqual(await older, []);
    assert.equal(isFetching(), true);
    assert.equal(f.requests.length, 1);
    assert.equal(f.requests[0].baseUrl, "https://new.invalid/v1");
    f.requests[0].response.resolve(["new"]);
    assert.deepEqual(await newer, ["new"]);
  },
);

check(
  "an obsolete active response cannot dismiss a queued refresh",
  async (f) => {
    const older = f.fetch();
    const gate = f.delay("change_post_process_base_url_setting");
    const saving = f.track(
      store().updatePostProcessBaseUrl("custom", "https://new.invalid/v1"),
    );
    const queued = f.fetch();
    f.requests[0].response.resolve(["old"]);
    assert.deepEqual(await older, []);
    assert.equal(isFetching(), true);
    assert.deepEqual(modelOptions(), []);
    gate.resolve();
    await saving;
    assert.equal(f.requests[1].baseUrl, "https://new.invalid/v1");
    f.requests[1].response.resolve(["new"]);
    assert.deepEqual(await queued, ["new"]);
  },
);

check(
  "changing one provider does not invalidate another provider's request",
  async (f) => {
    const custom = f.fetch();
    const other = f.fetch("openai");
    await f.track(
      store().updatePostProcessBaseUrl("custom", "https://new.invalid/v1"),
    );
    f.requests[0].response.resolve(["old"]);
    await custom;
    assert.equal(isFetching("openai"), true);
    f.requests[1].response.resolve(["other"]);
    assert.deepEqual(await other, ["other"]);
    assert.deepEqual(modelOptions("openai"), ["other"]);
  },
);

check(
  "reselecting a provider invalidates its earlier list request",
  async (f) => {
    const older = f.fetch();
    await f.track(store().setPostProcessProvider("custom"));
    const newer = f.fetch();
    f.requests[0].response.resolve(["old"]);
    await older;
    assert.equal(isFetching(), true);
    f.requests[1].response.resolve(["new"]);
    await newer;
    assert.deepEqual(modelOptions(), ["new"]);
  },
);

check(
  "a settings refresh changing the connection makes the old response unusable",
  async (f) => {
    const pending = f.fetch();
    f.settings.post_process_providers![0].base_url = "https://new.invalid/v1";
    await f.track(store().refreshSettings());
    f.requests[0].response.resolve(["old"]);
    assert.deepEqual(await pending, []);
    assert.deepEqual(modelOptions(), []);
    assert.equal(isFetching(), false);
  },
);

check(
  "an unrelated settings refresh preserves a valid pending response",
  async (f) => {
    const pending = f.fetch();
    f.settings.theme = "dark";
    await f.track(store().refreshSettings());
    f.requests[0].response.resolve(["current"]);
    assert.deepEqual(await pending, ["current"]);
  },
);

for (const field of ["api_format", "api_key"] as const) {
  check(
    `a settings refresh changing ${field} invalidates old cached and pending models`,
    async (f) => {
      store().setPostProcessModelOptions("custom", ["cached"]);
      const pending = f.fetch();
      if (field === "api_format")
        f.settings.post_process_providers![0].api_format = "anthropic";
      else f.settings.post_process_api_keys!.custom = "test-new-key";
      await f.track(store().refreshSettings());
      assert.deepEqual(modelOptions(), []);
      f.requests[0].response.resolve(["old"]);
      assert.deepEqual(await pending, []);
      assert.deepEqual(modelOptions(), []);
    },
  );
}

check(
  "clearing settings prevents an in-flight response from restoring model choices",
  async (f) => {
    const pending = f.fetch();
    store().setSettings(null);
    f.requests[0].response.resolve(["old"]);
    assert.deepEqual(await pending, []);
    assert.deepEqual(modelOptions(), []);
    assert.equal(isFetching(), false);
  },
);

check(
  "changing the selected model does not invalidate the connection's model list",
  async (f) => {
    const pending = f.fetch();
    await f.track(store().updatePostProcessModel("custom", "chosen-model"));
    f.requests[0].response.resolve(["current"]);
    assert.deepEqual(await pending, ["current"]);
  },
);

check(
  "a partial URL update failure still refreshes the effective connection",
  async (f) => {
    f.fail("change_post_process_model_setting", "model reset rejected");
    const gate = f.delay("change_post_process_model_setting");
    const saving = f.track(
      store().updatePostProcessBaseUrl("custom", "https://new.invalid/v1"),
    );
    const queued = f.fetch();
    assert.equal(f.requests.length, 0);
    gate.resolve();
    await saving;
    assert.equal(
      store().settings?.post_process_providers?.[0].base_url,
      "https://new.invalid/v1",
    );
    assert.equal(f.requests[0].baseUrl, "https://new.invalid/v1");
    f.requests[0].response.resolve(["new-server-model"]);
    assert.deepEqual(await queued, ["new-server-model"]);
  },
);

check(
  "a missing provider does not start a native model-list request",
  async (f) => {
    const missing = f.fetch("removed-provider");
    assert.equal(f.requests.length, 0);
    assert.deepEqual(await missing, []);
    assert.equal(isFetching("removed-provider"), false);
  },
);

check(
  "a current failure keeps the last valid list and permits a retry",
  async (f) => {
    store().setPostProcessModelOptions("custom", ["cached"]);
    const failing = f.fetch();
    f.requests[0].response.reject("request failed");
    assert.deepEqual(await failing, []);
    assert.deepEqual(modelOptions(), ["cached"]);
    assert.equal(isFetching(), false);
    const retry = f.fetch();
    f.requests[1].response.resolve(["retried"]);
    assert.deepEqual(await retry, ["retried"]);
  },
);

for (const error of ["save rejected", new Error("IPC failed")]) {
  check(
    `a queued refresh resumes after a failed connection save (${typeof error})`,
    async (f) => {
      f.fail("change_post_process_base_url_setting", error);
      const gate = f.delay("change_post_process_base_url_setting");
      const saving = f.track(
        store().updatePostProcessBaseUrl("custom", "https://new.invalid/v1"),
      );
      const queued = f.fetch();
      assert.equal(f.requests.length, 0);
      gate.resolve();
      await saving;
      assert.equal(f.requests[0].baseUrl, "https://old.invalid/v1");
      f.requests[0].response.resolve(["current"]);
      assert.deepEqual(await queued, ["current"]);
      assert.equal(isFetching(), false);
    },
  );
}

const failures: string[] = [];
try {
  for (const scenario of cases) {
    const f = fixture();
    try {
      await scenario.run(f);
      console.log(`PASS ${scenario.name}`);
    } catch (error) {
      failures.push(scenario.name);
      console.log(`FAIL ${scenario.name}: ${error}`);
    } finally {
      await f.cleanup();
    }
  }
} finally {
  clearMocks();
  useSettingsStore.setState(originalState, true);
  console.error = originalConsoleError;
  if (originalWindow)
    Object.defineProperty(globalThis, "window", originalWindow);
  else Reflect.deleteProperty(globalThis, "window");
}
console.log(
  `postProcessModels: ${cases.length - failures.length}/${cases.length} scenarios passed`,
);
assert.deepEqual(failures, []);
