import { mockIPC } from "@tauri-apps/api/mocks";
import type { AppSettings } from "@/bindings";
import defaultSettings from "./default-settings.json";
import { emit } from "@tauri-apps/api/event";
import type { WordsPeriod } from "@/bindings";
import * as wf from "./wordsFixtures";
import {
  modelsFixture,
  statsFixture,
  updateFixture,
  type UpdateFixtureKind,
} from "./fixtures";

export interface MockOptions {
  theme?: "light" | "dark" | "system";
  language?: string;
  /** Overrides merged into the settings the mocked backend returns. */
  settings?: Partial<AppSettings>;
  /** Returned by `get_voxa_stats`; the default fixture when omitted. */
  stats?: ReturnType<typeof statsFixture> | "error" | "empty";
  /** Update state the mocked backend reports. */
  update?: UpdateFixtureKind;
  /** Words tab: no words at all, every block failing, or the backfill running. */
  words?: "empty" | "error" | "backfill";
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
  let updateStatus = updateFixture(options.update ?? "idle");

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
          return false;
        case "voxa_update_status":
          return updateStatus;
        case "voxa_check_for_updates":
          // A check on a fresh preview finds the announced update or reports
          // up to date, like the real backend would.
          updateStatus =
            updateStatus.phase === "idle"
              ? updateFixture("up_to_date")
              : updateStatus;
          return updateStatus;
        case "voxa_install_update":
          updateStatus = updateFixture("downloading");
          void emit("voxa-update-status", updateStatus);
          return null;
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
        case "voxa_words_top":
        case "voxa_words_new":
        case "voxa_words_vocabulary":
        case "voxa_words_openers":
        case "voxa_words_phrases":
        case "voxa_words_fillers":
        case "voxa_word_detail": {
          if (options.words === "error") throw "database is locked";
          const a = args as {
            period: WordsPeriod;
            includeStop?: boolean;
            stem?: string;
          };
          const empty = options.words === "empty";
          switch (cmd) {
            case "voxa_words_top":
              return empty ? [] : wf.topWords(a.period, !!a.includeStop);
            case "voxa_words_new":
              return empty ? [] : wf.newWords();
            case "voxa_words_vocabulary":
              return empty
                ? { distinct: 0, total: 0 }
                : wf.vocabulary(a.period);
            case "voxa_words_openers":
              return empty
                ? { total: 0, one: [], two: [] }
                : wf.openers(a.period);
            case "voxa_words_phrases":
              return empty
                ? { bigrams: [], trigrams: [] }
                : wf.phrases(a.period);
            case "voxa_words_fillers":
              return empty ? { items: [], weeks: [] } : wf.fillers(a.period);
            default:
              return wf.wordDetail(a.stem ?? "");
          }
        }
        case "voxa_words_backfill_status":
          return wf.backfillStatus(
            options.words === "backfill" ? "running" : "done",
          );
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
