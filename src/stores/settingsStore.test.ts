// Standalone regression check: bun run test:settings-store
import assert from "node:assert/strict";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { AppSettings } from "@/bindings";
import { useSettingsStore } from "./settingsStore";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: string) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

const originalState = useSettingsStore.getState();
const originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
const originalConsoleError = console.error;
const loggedErrors: unknown[][] = [];
Object.defineProperty(globalThis, "window", {
  value: {},
  configurable: true,
});
console.error = (...args: unknown[]) => loggedErrors.push(args);

function fixture() {
  useSettingsStore.setState(originalState, true);
  // Only these fields are read by the settings updates exercised below.
  const settings = {
    selected_channel: null,
    audio_feedback: true,
    theme: "light",
  } as AppSettings;
  useSettingsStore.getState().setSettings(settings);
  const channel = deferred<null>();
  const feedback = deferred<null>();
  mockIPC((command, args) => {
    if (command === "set_selected_channel") {
      assert.deepEqual(args, { channel: 1 });
      return channel.promise;
    }
    if (command === "change_audio_feedback_setting") {
      assert.deepEqual(args, { enabled: false });
      return feedback.promise;
    }
    throw new Error(`Unexpected command: ${command}`);
  });
  return { settings, channel, feedback };
}

const store = () => useSettingsStore.getState();
const startChannelChange = () => store().updateSetting("selected_channel", 1);
const startFeedbackChange = () =>
  store().updateSetting("audio_feedback", false);

function assertRolledBackChannel() {
  assert.equal(store().settings?.selected_channel, null);
  assert.equal(store().isUpdatingKey("selected_channel"), false);
}

try {
  // A delayed failure must not undo another setting's successful update.
  let f = fixture();
  let channelChange = startChannelChange();
  let feedbackChange = startFeedbackChange();
  f.feedback.resolve(null);
  await feedbackChange;
  f.channel.reject("channel switch rejected");
  await channelChange;
  assertRolledBackChannel();
  assert.equal(store().settings?.audio_feedback, false);

  // The same change must survive even if its request is still pending.
  f = fixture();
  channelChange = startChannelChange();
  feedbackChange = startFeedbackChange();
  f.channel.reject("channel switch rejected");
  await channelChange;
  assertRolledBackChannel();
  assert.equal(store().settings?.audio_feedback, false);
  assert.equal(store().isUpdatingKey("audio_feedback"), true);
  f.feedback.resolve(null);
  await feedbackChange;
  assert.equal(store().settings?.audio_feedback, false);
  assert.equal(store().isUpdatingKey("audio_feedback"), false);

  // Preserve unrelated values supplied by a settings refresh while waiting.
  f = fixture();
  channelChange = startChannelChange();
  store().setSettings({ ...f.settings, theme: "dark" });
  f.channel.reject("channel switch rejected");
  await channelChange;
  assertRolledBackChannel();
  assert.equal(store().settings?.theme, "dark");

  // A late failure must not restore a settings object that has been cleared.
  f = fixture();
  channelChange = startChannelChange();
  store().setSettings(null);
  f.channel.reject("channel switch rejected");
  await channelChange;
  assert.equal(store().settings, null);

  // A failure without concurrent changes still restores the original value.
  f = fixture();
  channelChange = startChannelChange();
  f.channel.reject("channel switch rejected");
  await channelChange;
  assertRolledBackChannel();
  assert.equal(store().settings?.audio_feedback, true);

  // Successful requests still retain both changes and clear their busy flags.
  f = fixture();
  channelChange = startChannelChange();
  feedbackChange = startFeedbackChange();
  f.channel.resolve(null);
  f.feedback.resolve(null);
  await Promise.all([channelChange, feedbackChange]);
  assert.equal(store().settings?.selected_channel, 1);
  assert.equal(store().settings?.audio_feedback, false);
  assert.equal(store().isUpdatingKey("selected_channel"), false);
  assert.equal(store().isUpdatingKey("audio_feedback"), false);
  assert.equal(loggedErrors.length, 5);
} finally {
  clearMocks();
  useSettingsStore.setState(originalState, true);
  console.error = originalConsoleError;
  if (originalWindow) {
    Object.defineProperty(globalThis, "window", originalWindow);
  } else {
    Reflect.deleteProperty(globalThis, "window");
  }
}

console.log("settingsStore: all 6 regression scenarios passed");
