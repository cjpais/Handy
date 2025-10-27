import { create } from "zustand";
import { subscribeWithSelector } from "zustand/middleware";
import { invoke } from "@tauri-apps/api/core";
import { Settings, AudioDevice, PostProcessProvider } from "../lib/types";

interface SettingsStore {
  settings: Settings | null;
  isLoading: boolean;
  isUpdating: Record<string, boolean>;
  audioDevices: AudioDevice[];
  outputDevices: AudioDevice[];
  customSounds: { start: boolean; stop: boolean };
  postProcessModelOptions: Record<string, string[]>;

  // Actions
  initialize: () => Promise<void>;
  updateSetting: <K extends keyof Settings>(
    key: K,
    value: Settings[K],
  ) => Promise<void>;
  resetSetting: (key: keyof Settings) => Promise<void>;
  refreshSettings: () => Promise<void>;
  refreshAudioDevices: () => Promise<void>;
  refreshOutputDevices: () => Promise<void>;
  updateBinding: (id: string, binding: string) => Promise<void>;
  resetBinding: (id: string) => Promise<void>;
  getSetting: <K extends keyof Settings>(key: K) => Settings[K] | undefined;
  isUpdatingKey: (key: string) => boolean;
  playTestSound: (soundType: "start" | "stop") => Promise<void>;
  checkCustomSounds: () => Promise<void>;
  setPostProcessProvider: (providerId: string) => Promise<void>;
  updatePostProcessBaseUrl: (providerId: string, baseUrl: string) => Promise<void>;
  updatePostProcessApiKey: (providerId: string, apiKey: string) => Promise<void>;
  updatePostProcessModel: (providerId: string, model: string) => Promise<void>;
  fetchPostProcessModels: (providerId: string) => Promise<string[]>;
  setPostProcessModelOptions: (providerId: string, models: string[]) => void;

  // Internal state setters
  setSettings: (settings: Settings | null) => void;
  setLoading: (loading: boolean) => void;
  setUpdating: (key: string, updating: boolean) => void;
  setAudioDevices: (devices: AudioDevice[]) => void;
  setOutputDevices: (devices: AudioDevice[]) => void;
  setCustomSounds: (sounds: { start: boolean; stop: boolean }) => void;
}

const DEFAULT_POST_PROCESS_PROVIDERS: PostProcessProvider[] = [
  {
    id: "openai",
    label: "OpenAI",
    base_url: "https://api.openai.com/v1",
    allow_base_url_edit: false,
    models_endpoint: "/models",
    kind: "openai_compatible",
  },
  {
    id: "openrouter",
    label: "OpenRouter",
    base_url: "https://openrouter.ai/api/v1",
    allow_base_url_edit: false,
    models_endpoint: "/models",
    kind: "openai_compatible",
  },
  {
    id: "anthropic",
    label: "Anthropic",
    base_url: "https://api.anthropic.com/v1",
    allow_base_url_edit: false,
    models_endpoint: "/models",
    kind: "anthropic",
  },
  {
    id: "custom",
    label: "Custom",
    base_url: "http://localhost:11434/v1",
    allow_base_url_edit: true,
    models_endpoint: "/models",
    kind: "openai_compatible",
  },
];

const DEFAULT_SETTINGS: Partial<Settings> = {
  always_on_microphone: false,
  audio_feedback: true,
  audio_feedback_volume: 1.0,
  sound_theme: "marimba",
  start_hidden: false,
  autostart_enabled: false,
  push_to_talk: false,
  selected_microphone: "Default",
  selected_output_device: "Default",
  translate_to_english: false,
  selected_language: "auto",
  overlay_position: "bottom",
  debug_mode: false,
  custom_words: [],
  history_limit: 5,
  post_process_enabled: false,
  post_process_provider_id: "openai",
  post_process_providers: DEFAULT_POST_PROCESS_PROVIDERS,
  post_process_api_keys: {
    openai: "",
    openrouter: "",
    anthropic: "",
    custom: "",
  },
  post_process_models: {
    openai: "",
    openrouter: "",
    anthropic: "",
    custom: "",
  },
  post_process_prompts: [],
  post_process_selected_prompt_id: null,
};

const DEFAULT_AUDIO_DEVICE: AudioDevice = {
  index: "default",
  name: "Default",
  is_default: true,
};

const settingUpdaters: {
  [K in keyof Settings]?: (value: Settings[K]) => Promise<unknown>;
} = {
  always_on_microphone: (value) =>
    invoke("update_microphone_mode", { alwaysOn: value }),
  audio_feedback: (value) =>
    invoke("change_audio_feedback_setting", { enabled: value }),
  audio_feedback_volume: (value) =>
    invoke("change_audio_feedback_volume_setting", { volume: value }),
  sound_theme: (value) =>
    invoke("change_sound_theme_setting", { theme: value }),
  start_hidden: (value) =>
    invoke("change_start_hidden_setting", { enabled: value }),
  autostart_enabled: (value) =>
    invoke("change_autostart_setting", { enabled: value }),
  push_to_talk: (value) => invoke("change_ptt_setting", { enabled: value }),
  selected_microphone: (value) =>
    invoke("set_selected_microphone", {
      deviceName: value === "Default" ? "default" : value,
    }),
  selected_output_device: (value) =>
    invoke("set_selected_output_device", {
      deviceName: value === "Default" ? "default" : value,
    }),
  translate_to_english: (value) =>
    invoke("change_translate_to_english_setting", { enabled: value }),
  selected_language: (value) =>
    invoke("change_selected_language_setting", { language: value }),
  overlay_position: (value) =>
    invoke("change_overlay_position_setting", { position: value }),
  debug_mode: (value) =>
    invoke("change_debug_mode_setting", { enabled: value }),
  custom_words: (value) => invoke("update_custom_words", { words: value }),
  word_correction_threshold: (value) =>
    invoke("change_word_correction_threshold_setting", { threshold: value }),
  paste_method: (value) =>
    invoke("change_paste_method_setting", { method: value }),
  clipboard_handling: (value) =>
    invoke("change_clipboard_handling_setting", { handling: value }),
  history_limit: (value) => invoke("update_history_limit", { limit: value }),
  post_process_enabled: (value) =>
    invoke("change_post_process_enabled_setting", { enabled: value }),
  post_process_selected_prompt_id: (value) =>
    invoke("set_post_process_selected_prompt", { id: value }),
};

export const useSettingsStore = create<SettingsStore>()(
  subscribeWithSelector((set, get) => ({
    settings: null,
    isLoading: true,
    isUpdating: {},
    audioDevices: [],
    outputDevices: [],
    customSounds: { start: false, stop: false },
    postProcessModelOptions: {},

    // Internal setters
    setSettings: (settings) => set({ settings }),
    setLoading: (isLoading) => set({ isLoading }),
    setUpdating: (key, updating) =>
      set((state) => ({
        isUpdating: { ...state.isUpdating, [key]: updating },
      })),
    setAudioDevices: (audioDevices) => set({ audioDevices }),
    setOutputDevices: (outputDevices) => set({ outputDevices }),
    setCustomSounds: (customSounds) => set({ customSounds }),

    // Getters
    getSetting: (key) => get().settings?.[key],
    isUpdatingKey: (key) => get().isUpdating[key] || false,

    // Load settings from store
    refreshSettings: async () => {
      try {
        const { load } = await import("@tauri-apps/plugin-store");
        const store = await load("settings_store.json", {
          autoSave: false,
          defaults: {},
        });
        const settings = (await store.get("settings")) as Settings;

        // Load additional settings that come from invoke calls
        const [microphoneMode, selectedMicrophone, selectedOutputDevice] =
          await Promise.allSettled([
            invoke("get_microphone_mode"),
            invoke("get_selected_microphone"),
            invoke("get_selected_output_device"),
          ]);

        // Merge all settings
        const mergedSettings: Settings = {
          ...settings,
          always_on_microphone:
            microphoneMode.status === "fulfilled"
              ? (microphoneMode.value as boolean)
              : false,
          selected_microphone:
            selectedMicrophone.status === "fulfilled"
              ? (selectedMicrophone.value as string)
              : "Default",
          selected_output_device:
            selectedOutputDevice.status === "fulfilled"
              ? (selectedOutputDevice.value as string)
              : "Default",
        };

        set({ settings: mergedSettings, isLoading: false });
      } catch (error) {
        console.error("Failed to load settings:", error);
        set({ isLoading: false });
      }
    },

    // Load audio devices
    refreshAudioDevices: async () => {
      try {
        const devices: AudioDevice[] = await invoke(
          "get_available_microphones",
        );
        const devicesWithDefault = [
          DEFAULT_AUDIO_DEVICE,
          ...devices.filter(
            (d) => d.name !== "Default" && d.name !== "default",
          ),
        ];
        set({ audioDevices: devicesWithDefault });
      } catch (error) {
        console.error("Failed to load audio devices:", error);
        set({ audioDevices: [DEFAULT_AUDIO_DEVICE] });
      }
    },

    // Load output devices
    refreshOutputDevices: async () => {
      try {
        const devices: AudioDevice[] = await invoke(
          "get_available_output_devices",
        );
        const devicesWithDefault = [
          DEFAULT_AUDIO_DEVICE,
          ...devices.filter(
            (d) => d.name !== "Default" && d.name !== "default",
          ),
        ];
        set({ outputDevices: devicesWithDefault });
      } catch (error) {
        console.error("Failed to load output devices:", error);
        set({ outputDevices: [DEFAULT_AUDIO_DEVICE] });
      }
    },

    // Play a test sound
    playTestSound: async (soundType: "start" | "stop") => {
      try {
        await invoke("play_test_sound", { soundType });
      } catch (error) {
        console.error(`Failed to play test sound (${soundType}):`, error);
      }
    },


    checkCustomSounds: async () => {
      try {
        const sounds = await invoke("check_custom_sounds");
        get().setCustomSounds(sounds as { start: boolean; stop: boolean });
      } catch (error) {
        console.error("Failed to check custom sounds:", error);
      }
    },

    // Update a specific setting
    updateSetting: async <K extends keyof Settings>(
      key: K,
      value: Settings[K],
    ) => {
      const { settings, setUpdating } = get();
      const updateKey = String(key);
      const originalValue = settings?.[key];

      setUpdating(updateKey, true);

      try {
        set((state) => ({
          settings: state.settings ? { ...state.settings, [key]: value } : null,
        }));

        const updater = settingUpdaters[key];
        if (updater) {
          await updater(value);
        } else if (key !== "bindings" && key !== "selected_model") {
          console.warn(`No handler for setting: ${String(key)}`);
        }
      } catch (error) {
        console.error(`Failed to update setting ${String(key)}:`, error);
        if (settings) {
          set({ settings: { ...settings, [key]: originalValue } });
        }
      } finally {
        setUpdating(updateKey, false);
      }
    },

    // Reset a setting to its default value
    resetSetting: async (key) => {
      const defaultValue = DEFAULT_SETTINGS[key];
      if (defaultValue !== undefined) {
        await get().updateSetting(key, defaultValue as any);
      }
    },

    // Update a specific binding
    updateBinding: async (id, binding) => {
      const { settings, setUpdating } = get();
      const updateKey = `binding_${id}`;
      const originalBinding = settings?.bindings?.[id]?.current_binding;

      setUpdating(updateKey, true);

      try {
        // Optimistic update
        set((state) => ({
          settings: state.settings
            ? {
                ...state.settings,
                bindings: {
                  ...state.settings.bindings,
                  [id]: {
                    ...state.settings.bindings[id],
                    current_binding: binding,
                  },
                },
              }
            : null,
        }));

        await invoke("change_binding", { id, binding });
      } catch (error) {
        console.error(`Failed to update binding ${id}:`, error);

        // Rollback on error
        if (originalBinding && get().settings) {
          set((state) => ({
            settings: state.settings
              ? {
                  ...state.settings,
                  bindings: {
                    ...state.settings.bindings,
                    [id]: {
                      ...state.settings.bindings[id],
                      current_binding: originalBinding,
                    },
                  },
                }
              : null,
          }));
        }
      } finally {
        setUpdating(updateKey, false);
      }
    },

    // Reset a specific binding
    resetBinding: async (id) => {
      const { setUpdating, refreshSettings } = get();
      const updateKey = `binding_${id}`;

      setUpdating(updateKey, true);

      try {
        await invoke("reset_binding", { id });
        await refreshSettings();
      } catch (error) {
        console.error(`Failed to reset binding ${id}:`, error);
      } finally {
        setUpdating(updateKey, false);
      }
    },

    setPostProcessProvider: async (providerId) => {
      const { settings, setUpdating, refreshSettings } = get();
      const updateKey = "post_process_provider_id";
      const previousId = settings?.post_process_provider_id ?? null;

      setUpdating(updateKey, true);

      if (settings) {
        set((state) => ({
          settings: state.settings
            ? { ...state.settings, post_process_provider_id: providerId }
            : null,
        }));
      }

      try {
        await invoke("set_post_process_provider", { providerId });
        await refreshSettings();
      } catch (error) {
        console.error("Failed to set post-process provider:", error);
        if (previousId !== null) {
          set((state) => ({
            settings: state.settings
              ? { ...state.settings, post_process_provider_id: previousId }
              : null,
          }));
        }
      } finally {
        setUpdating(updateKey, false);
      }
    },

    updatePostProcessBaseUrl: async (providerId, baseUrl) => {
      const { setUpdating, refreshSettings } = get();
      const updateKey = `post_process_base_url:${providerId}`;

      setUpdating(updateKey, true);

      try {
        await invoke("change_post_process_base_url_setting", {
          providerId,
          baseUrl,
        });
        await refreshSettings();
      } catch (error) {
        console.error("Failed to update post-process base URL:", error);
      } finally {
        setUpdating(updateKey, false);
      }
    },

    updatePostProcessApiKey: async (providerId, apiKey) => {
      const { setUpdating, refreshSettings } = get();
      const updateKey = `post_process_api_key:${providerId}`;

      setUpdating(updateKey, true);

      try {
        await invoke("change_post_process_api_key_setting", {
          providerId,
          apiKey,
        });
        await refreshSettings();
      } catch (error) {
        console.error("Failed to update post-process API key:", error);
      } finally {
        setUpdating(updateKey, false);
      }
    },

    updatePostProcessModel: async (providerId, model) => {
      const { setUpdating, refreshSettings } = get();
      const updateKey = `post_process_model:${providerId}`;

      setUpdating(updateKey, true);

      try {
        await invoke("change_post_process_model_setting", {
          providerId,
          model,
        });
        await refreshSettings();
      } catch (error) {
        console.error("Failed to update post-process model:", error);
      } finally {
        setUpdating(updateKey, false);
      }
    },

    fetchPostProcessModels: async (providerId) => {
      const updateKey = `post_process_models_fetch:${providerId}`;
      const {
        setUpdating,
        setPostProcessModelOptions,
        refreshSettings,
        settings,
      } = get();

      setUpdating(updateKey, true);

      try {
        if (!settings) {
          await refreshSettings();
        }

        const currentSettings = get().settings;
        const providers =
          currentSettings?.post_process_providers?.length
            ? currentSettings.post_process_providers
            : DEFAULT_POST_PROCESS_PROVIDERS;

        const provider = providers.find((p) => p.id === providerId);
        if (!provider) {
          throw new Error(`Provider '${providerId}' not found`);
        }

        const baseUrl = (provider.base_url || "").replace(/\/$/, "");
        if (!baseUrl) {
          throw new Error("Provider base URL is not configured.");
        }
        const endpointPath = (provider.models_endpoint || "/models").replace(
          /^\//,
          "",
        );
        const endpoint = `${baseUrl}/${endpointPath}`;

        const headers: Record<string, string> = {
          "HTTP-Referer": "https://github.com/cjpais/Handy",
          "X-Title": "Handy",
        };

        const apiKey =
          currentSettings?.post_process_api_keys?.[providerId]?.trim() || "";

        if (provider.kind === "anthropic") {
          if (!apiKey) {
            throw new Error(
              "An Anthropic API key is required to list available models.",
            );
          }
          headers["x-api-key"] = apiKey;
          headers["anthropic-version"] = "2023-06-01";
        } else if (apiKey) {
          headers.Authorization = `Bearer ${apiKey}`;
        }

        const response = await fetch(endpoint, {
          method: "GET",
          headers,
        });

        if (!response.ok) {
          const errorText = await response.text();
          throw new Error(
            `Model list request failed (${response.status}): ${errorText || "unknown error"}`,
          );
        }

        let parsed: unknown;
        try {
          parsed = await response.json();
        } catch (error) {
          throw new Error(
            `Failed to parse model list response: ${(error as Error).message}`,
          );
        }

        const collected: Array<{ id: string; created?: number }> = [];

        if (
          parsed &&
          typeof parsed === "object" &&
          Array.isArray((parsed as { data?: unknown[] }).data)
        ) {
          for (const entry of (parsed as { data: any[] }).data) {
            if (entry && typeof entry === "object") {
              const id = typeof entry.id === "string" ? entry.id : undefined;
              const name = typeof entry.name === "string" ? entry.name : undefined;
              const created =
                typeof entry.created === "number"
                  ? entry.created
                  : undefined;
              const identifier = id || name;
              if (identifier) {
                collected.push({ id: identifier, created });
              }
            }
          }
        } else if (Array.isArray(parsed)) {
          for (const entry of parsed) {
            if (typeof entry === "string") {
              collected.push({ id: entry });
            }
          }
        }

        const dedup = new Map<string, number | undefined>();
        for (const { id, created } of collected) {
          const existing = dedup.get(id);
          if (existing === undefined) {
            dedup.set(id, created);
          } else if (
            created !== undefined &&
            (existing === undefined || created > existing)
          ) {
            dedup.set(id, created);
          }
        }

        const models = Array.from(dedup.entries())
          .sort(([idA, createdA], [idB, createdB]) => {
            if (createdA !== undefined && createdB !== undefined) {
              return createdB - createdA;
            }
            if (createdA !== undefined) return -1;
            if (createdB !== undefined) return 1;
            return idA.localeCompare(idB);
          })
          .map(([id]) => id);

        setPostProcessModelOptions(providerId, models);
        return models;
      } catch (error) {
        console.error("Failed to fetch models:", error);
        setPostProcessModelOptions(providerId, []);
        return [];
      } finally {
        setUpdating(updateKey, false);
      }
    },

    setPostProcessModelOptions: (providerId, models) =>
      set((state) => ({
        postProcessModelOptions: {
          ...state.postProcessModelOptions,
          [providerId]: models,
        },
      })),

    // Initialize everything
    initialize: async () => {
      const {
        refreshSettings,
        refreshAudioDevices,
        refreshOutputDevices,
        checkCustomSounds,
      } = get();
      await Promise.all([
        refreshSettings(),
        refreshAudioDevices(),
        refreshOutputDevices(),
        checkCustomSounds(),
      ]);
    },
  })),
);
