import { create } from "zustand";
import { subscribeWithSelector } from "zustand/middleware";
import { invoke } from "@tauri-apps/api/core";
import { Settings, AudioDevice, RegexFilter, PolishRule } from "../lib/types";

interface SettingsStore {
  settings: Settings | null;
  isLoading: boolean;
  isUpdating: Record<string, boolean>;
  audioDevices: AudioDevice[];
  outputDevices: AudioDevice[];

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

  // Regex filter actions
  getRegexFilters: () => Promise<RegexFilter[]>;
  addRegexFilter: (name: string, pattern: string, replacement: string) => Promise<RegexFilter>;
  updateRegexFilter: (id: string, name: string, pattern: string, replacement: string, enabled: boolean) => Promise<void>;
  deleteRegexFilter: (id: string) => Promise<void>;
  toggleRegexFilter: (id: string, enabled: boolean) => Promise<void>;

  // Polish rule actions
  getPolishRules: () => Promise<PolishRule[]>;
  addPolishRule: (name: string, api_url: string, api_key: string, model: string, prompt: string) => Promise<PolishRule>;
  updatePolishRule: (id: string, name: string, api_url: string, api_key: string, model: string, prompt: string, enabled: boolean) => Promise<void>;
  deletePolishRule: (id: string) => Promise<void>;
  togglePolishRule: (id: string, enabled: boolean) => Promise<void>;

  // Internal state setters
  setSettings: (settings: Settings | null) => void;
  setLoading: (loading: boolean) => void;
  setUpdating: (key: string, updating: boolean) => void;
  setAudioDevices: (devices: AudioDevice[]) => void;
  setOutputDevices: (devices: AudioDevice[]) => void;
}

const DEFAULT_SETTINGS: Partial<Settings> = {
  always_on_microphone: false,
  audio_feedback: true,
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
  initial_prompt: "",
  regex_filters: [],
  polish_rules: [],
  auto_polish: false,
};

const DEFAULT_AUDIO_DEVICE: AudioDevice = {
  index: "default",
  name: "Default",
  is_default: true,
};

export const useSettingsStore = create<SettingsStore>()(
  subscribeWithSelector((set, get) => ({
    settings: null,
    isLoading: true,
    isUpdating: {},
    audioDevices: [],
    outputDevices: [],

    // Internal setters
    setSettings: (settings) => set({ settings }),
    setLoading: (isLoading) => set({ isLoading }),
    setUpdating: (key, updating) =>
      set((state) => ({
        isUpdating: { ...state.isUpdating, [key]: updating },
      })),
    setAudioDevices: (audioDevices) => set({ audioDevices }),
    setOutputDevices: (outputDevices) => set({ outputDevices }),

    // Getters
    getSetting: (key) => get().settings?.[key],
    isUpdatingKey: (key) => get().isUpdating[key] || false,

    // Load settings from store
    refreshSettings: async () => {
      try {
        const { load } = await import("@tauri-apps/plugin-store");
        const store = await load("settings_store.json", { autoSave: false });
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

    // Update a specific setting
    updateSetting: async <K extends keyof Settings>(
      key: K,
      value: Settings[K],
    ) => {
      const { settings, setUpdating, refreshSettings } = get();
      const updateKey = String(key);
      const originalValue = settings?.[key];

      setUpdating(updateKey, true);

      try {
        // Optimistic update
        set((state) => ({
          settings: state.settings ? { ...state.settings, [key]: value } : null,
        }));

        // Invoke the appropriate backend method
        switch (key) {
          case "always_on_microphone":
            await invoke("update_microphone_mode", { alwaysOn: value });
            break;
          case "audio_feedback":
            await invoke("change_audio_feedback_setting", { enabled: value });
            break;
          case "start_hidden":
            await invoke("change_start_hidden_setting", { enabled: value });
            break;
          case "autostart_enabled":
            await invoke("change_autostart_setting", { enabled: value });
            break;
          case "push_to_talk":
            await invoke("change_ptt_setting", { enabled: value });
            break;
          case "selected_microphone":
            const micDeviceName = value === "Default" ? "default" : value;
            await invoke("set_selected_microphone", {
              deviceName: micDeviceName,
            });
            break;
          case "selected_output_device":
            const outputDeviceName = value === "Default" ? "default" : value;
            await invoke("set_selected_output_device", {
              deviceName: outputDeviceName,
            });
            break;
          case "translate_to_english":
            await invoke("change_translate_to_english_setting", {
              enabled: value,
            });
            break;
          case "selected_language":
            await invoke("change_selected_language_setting", {
              language: value,
            });
            break;
          case "overlay_position":
            await invoke("change_overlay_position_setting", {
              position: value,
            });
            break;
          case "debug_mode":
            await invoke("change_debug_mode_setting", { enabled: value });
            break;
          case "custom_words":
            await invoke("update_custom_words", { words: value });
            break;
          case "word_correction_threshold":
            await invoke("change_word_correction_threshold_setting", {
              threshold: value,
            });
            break;
          case "paste_method":
            await invoke("change_paste_method_setting", { method: value });
            break;
          case "history_limit":
            await invoke("update_history_limit", { limit: value });
            break;
          case "initial_prompt":
            await invoke("change_initial_prompt_setting", { prompt: value });
            break;
          case "auto_polish":
            await invoke("change_auto_polish_setting", { enabled: value });
            break;
          case "bindings":
          case "selected_model":
            break;
          default:
            console.warn(`No handler for setting: ${String(key)}`);
        }
      } catch (error) {
        console.error(`Failed to update setting ${String(key)}:`, error);

        // Rollback on error
        set((state) => ({
          settings: state.settings
            ? { ...state.settings, [key]: originalValue }
            : null,
        }));
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

    // Initialize everything
    initialize: async () => {
      const { refreshSettings, refreshAudioDevices, refreshOutputDevices } =
        get();
      await Promise.all([
        refreshSettings(),
        refreshAudioDevices(),
        refreshOutputDevices(),
      ]);
    },

    // Regex filter methods
    getRegexFilters: async () => {
      try {
        const filters: RegexFilter[] = await invoke("get_regex_filters");
        return filters;
      } catch (error) {
        console.error("Failed to get regex filters:", error);
        return [];
      }
    },

    addRegexFilter: async (name, pattern, replacement) => {
      try {
        const filter: RegexFilter = await invoke("add_regex_filter", {
          name,
          pattern,
          replacement,
        });
        
        // Update settings to include the new filter
        const { refreshSettings } = get();
        await refreshSettings();
        
        return filter;
      } catch (error) {
        console.error("Failed to add regex filter:", error);
        throw error;
      }
    },

    updateRegexFilter: async (id, name, pattern, replacement, enabled) => {
      try {
        await invoke("update_regex_filter", {
          id,
          name,
          pattern,
          replacement,
          enabled,
        });
        
        // Update settings to reflect the changes
        const { refreshSettings } = get();
        await refreshSettings();
      } catch (error) {
        console.error("Failed to update regex filter:", error);
        throw error;
      }
    },

    deleteRegexFilter: async (id) => {
      try {
        await invoke("delete_regex_filter", { id });
        
        // Update settings to remove the deleted filter
        const { refreshSettings } = get();
        await refreshSettings();
      } catch (error) {
        console.error("Failed to delete regex filter:", error);
        throw error;
      }
    },

    toggleRegexFilter: async (id, enabled) => {
      try {
        await invoke("toggle_regex_filter", { id, enabled });
        
        // Update settings to reflect the toggle
        const { refreshSettings } = get();
        await refreshSettings();
      } catch (error) {
        console.error("Failed to toggle regex filter:", error);
        throw error;
      }
    },

    getPolishRules: async () => {
      try {
        const rules = await invoke<PolishRule[]>("get_polish_rules");
        return rules;
      } catch (error) {
        console.error("Failed to get polish rules:", error);
        throw error;
      }
    },

    addPolishRule: async (name, api_url, api_key, model, prompt) => {
      try {
        const rule = await invoke<PolishRule>("add_polish_rule", {
          name,
          apiUrl: api_url,
          apiKey: api_key,
          model,
          prompt,
        });
        
        // Update settings to reflect the new rule
        const { refreshSettings } = get();
        await refreshSettings();
        
        return rule;
      } catch (error) {
        console.error("Failed to add polish rule:", error);
        throw error;
      }
    },

    updatePolishRule: async (id, name, api_url, api_key, model, prompt, enabled) => {
      try {
        await invoke("update_polish_rule", {
          id,
          name,
          apiUrl: api_url,
          apiKey: api_key,
          model,
          prompt,
          enabled,
        });
        
        // Update settings to reflect the changes
        const { refreshSettings } = get();
        await refreshSettings();
      } catch (error) {
        console.error("Failed to update polish rule:", error);
        throw error;
      }
    },

    deletePolishRule: async (id) => {
      try {
        await invoke("delete_polish_rule", { id });
        
        // Update settings to reflect the deletion
        const { refreshSettings } = get();
        await refreshSettings();
      } catch (error) {
        console.error("Failed to delete polish rule:", error);
        throw error;
      }
    },

    togglePolishRule: async (id, enabled) => {
      try {
        await invoke("toggle_polish_rule", { id, enabled });
        
        // Update settings to reflect the toggle
        const { refreshSettings } = get();
        await refreshSettings();
      } catch (error) {
        console.error("Failed to toggle polish rule:", error);
        throw error;
      }
    },
  })),
);
