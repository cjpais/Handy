import { create } from "zustand";
import { subscribeWithSelector } from "zustand/middleware";
import { invoke } from "@tauri-apps/api/core";
import { Settings, AudioDevice } from "../lib/types";

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
  updatePostProcessSetting: (settingType: 'base_url' | 'api_key' | 'model', providerId: string, value: string) => Promise<void>;
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

// Note: Default post-processing settings are now managed in Rust
// Settings will always have providers after migration
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
  mute_while_recording: false,
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
  mute_while_recording: (value) =>
    invoke("change_mute_while_recording_setting", { enabled: value }),
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
          defaults: DEFAULT_SETTINGS,
          autoSave: false
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

    // Generic updater for post-processing provider settings
    updatePostProcessSetting: async (
      settingType: 'base_url' | 'api_key' | 'model',
      providerId: string,
      value: string
    ) => {
      const { setUpdating, refreshSettings } = get();
      const updateKey = `post_process_${settingType}:${providerId}`;

      // Map setting types to command names
      const commandMap = {
        base_url: 'change_post_process_base_url_setting',
        api_key: 'change_post_process_api_key_setting',
        model: 'change_post_process_model_setting',
      };

      // Map setting types to param names
      const paramMap = {
        base_url: 'baseUrl',
        api_key: 'apiKey',
        model: 'model',
      };

      setUpdating(updateKey, true);

      try {
        await invoke(commandMap[settingType], {
          providerId,
          [paramMap[settingType]]: value,
        });
        await refreshSettings();
      } catch (error) {
        console.error(`Failed to update post-process ${settingType.replace('_', ' ')}:`, error);
      } finally {
        setUpdating(updateKey, false);
      }
    },

    updatePostProcessBaseUrl: async (providerId, baseUrl) => {
      return get().updatePostProcessSetting('base_url', providerId, baseUrl);
    },

    updatePostProcessApiKey: async (providerId, apiKey) => {
      // Clear cached models when API key changes - user should click refresh after
      set((state) => ({
        postProcessModelOptions: {
          ...state.postProcessModelOptions,
          [providerId]: [],
        },
      }));
      return get().updatePostProcessSetting('api_key', providerId, apiKey);
    },

    updatePostProcessModel: async (providerId, model) => {
      return get().updatePostProcessSetting('model', providerId, model);
    },

    fetchPostProcessModels: async (providerId) => {
      const updateKey = `post_process_models_fetch:${providerId}`;
      const { setUpdating, setPostProcessModelOptions } = get();

      setUpdating(updateKey, true);

      try {
        // Call Tauri backend command instead of fetch
        const models: string[] = await invoke("fetch_post_process_models", {
          providerId,
        });

        setPostProcessModelOptions(providerId, models);
        return models;
      } catch (error) {
        console.error("Failed to fetch models:", error);
        // Don't cache empty array on error - let user retry
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
