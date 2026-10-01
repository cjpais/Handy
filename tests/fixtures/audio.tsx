import React from "react";
import "../../src/App.css";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { Toaster } from "sonner";
import en from "../../src/i18n/locales/en/translation.json";
import type { AppSettings } from "../../src/bindings";

const calls: { command: string; args: unknown }[] = [];
let resolveStart: (session: number) => void;
let nextSession = 1;
const settings = {
  selected_microphone: "Default",
  selected_channel: null,
  onboarding_completed: new URLSearchParams(location.search).has("app"),
  app_language: "en",
} as AppSettings;
Object.assign(window, {
  __TAURI_OS_PLUGIN_INTERNALS__: {
    platform: "linux",
    os_type: "linux",
    family: "unix",
  },
  audioTest: {
    calls,
    emit,
    resolveStart: (session: number) => resolveStart(session),
    unmount: () => root.unmount(),
    hide: () => {
      Object.defineProperty(document, "visibilityState", {
        configurable: true,
        value: "hidden",
      });
      document.dispatchEvent(new Event("visibilitychange"));
    },
  },
});
mockWindows("main");
mockIPC(
  (command, args) => {
    calls.push({ command, args });
    switch (command) {
      case "start_microphone_test":
        if (new URLSearchParams(location.search).has("delayed")) {
          return new Promise<number>((resolve) => {
            resolveStart = resolve;
          });
        }
        return nextSession++;
      case "get_microphone_channels":
        return 2;
      case "get_available_microphones":
      case "get_available_output_devices":
        return [{ index: "default", name: "Default", is_default: true }];
      case "show_main_window_command":
        Object.defineProperty(document, "visibilityState", {
          configurable: true,
          value: "visible",
        });
        document.dispatchEvent(new Event("visibilitychange"));
        return null;
      case "get_app_settings":
      case "get_default_settings":
        return settings;
      case "has_any_models_available":
        return false;
      case "get_available_models":
        return [];
      case "plugin:os|locale":
        return "en-US";
      case "plugin:app|version":
        return "0.9.7";
      default:
        return null;
    }
  },
  { shouldMockEvents: true },
);
await i18n.use(initReactI18next).init({
  lng: "en",
  resources: { en: { translation: en } },
  interpolation: { escapeValue: false },
});
const { useSettingsStore } = await import("../../src/stores/settingsStore");
useSettingsStore.setState({
  settings,
  isLoading: false,
  audioDevices: [{ index: "default", name: "Default", is_default: true }],
});
const root = createRoot(document.getElementById("root")!);
if (new URLSearchParams(location.search).has("app")) {
  const { default: App } = await import("../../src/App");
  root.render(<App />);
} else {
  const { MicrophoneSelector } = await import(
    "../../src/components/settings/MicrophoneSelector"
  );
  const { ChannelSelector } = await import(
    "../../src/components/settings/ChannelSelector"
  );
  root.render(
    <>
      <MicrophoneSelector />
      <ChannelSelector />
      <Toaster />
    </>,
  );
}
