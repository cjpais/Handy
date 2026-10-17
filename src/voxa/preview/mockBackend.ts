import { mockIPC } from "@tauri-apps/api/mocks";
import type { AppSettings } from "@/bindings";
import defaultSettings from "./default-settings.json";
import { modelsFixture, statsFixture } from "./fixtures";

export interface MockOptions {
  theme?: "light" | "dark" | "system";
  language?: string;
  /** Overrides merged into the settings the mocked backend returns. */
  settings?: Partial<AppSettings>;
  /** Returned by `get_voxa_stats`; the default fixture when omitted. */
  stats?: ReturnType<typeof statsFixture> | "error" | "empty";
}

const TURBO =
  "handy-computer/whisper-large-v3-turbo-gguf/whisper-large-v3-turbo-Q8_0.gguf";

/**
 * Stands in for the Tauri backend in a plain browser: the real app renders
 * against fixtures, so the preview page can screenshot the actual screens.
 */
export function installMockBackend(options: MockOptions = {}): void {
  const settings = {
    ...(defaultSettings as unknown as AppSettings),
    theme: options.theme ?? "system",
    app_language: options.language ?? "ru",
    ...options.settings,
  } as AppSettings;

  const unknown = new Set<string>();

  mockIPC(
    (cmd, args) => {
      switch (cmd) {
        case "get_app_settings":
        case "get_default_settings":
          return settings;
        case "get_available_models":
          return modelsFixture();
        case "get_model_info":
          return (
            modelsFixture().find(
              (m) => m.id === (args as { modelId?: string })?.modelId,
            ) ?? null
          );
        case "get_current_model":
          return TURBO;
        case "get_transcription_model_status":
        case "get_model_load_status":
          return { is_loaded: true, current_model: TURBO };
        case "is_model_loading":
        case "is_portable":
        case "is_recording":
        case "is_laptop":
          return false;
        case "is_update_checks_locked":
          return true;
        case "get_retired_models":
        case "get_available_microphones":
        case "get_available_output_devices":
          return [];
        case "get_history_entries":
          return { entries: [], has_more: false };
        case "get_available_accelerators":
          return { available: ["cpu"], gpu_devices: [] };
        case "get_voxa_stats":
          if (options.stats === "error") throw "database is locked";
          if (options.stats === "empty") {
            const empty = statsFixture();
            return {
              ...empty,
              today: { dictations: 0, words: 0, minutes: 0 },
              week: { dictations: 0, words: 0, minutes: 0 },
              all_time: { dictations: 0, words: 0, minutes: 0 },
              daily: empty.daily.map((d) => ({
                ...d,
                words: 0,
                dictations: 0,
              })),
              models: [],
              candidates: [],
              latin_share: null,
              avg_seconds_per_dictation: null,
              avg_words_per_dictation: 0,
            };
          }
          return options.stats ?? statsFixture();
        case "voxa_get_handy_import":
          return null;
        case "voxa_add_dictionary_word":
          return [
            ...(settings.custom_words ?? []),
            (args as { word: string }).word,
          ];
        case "plugin:app|version":
          return "0.9.8";
        case "plugin:app|name":
          return "Voxa";
        case "plugin:path|resolve_directory":
          return "/Users/preview";
        default:
          if (cmd.startsWith("plugin:macos-permissions|")) return true;
          if (
            cmd.startsWith("plugin:event|") ||
            cmd.startsWith("plugin:window|")
          )
            return null;
          if (!unknown.has(cmd)) {
            unknown.add(cmd);
            console.debug(`[voxa-preview] unmocked command: ${cmd}`);
          }
          return null;
      }
    },
    { shouldMockEvents: true },
  );
}
