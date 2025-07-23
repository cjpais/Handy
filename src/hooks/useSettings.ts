import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Settings, AudioDevice } from "../lib/types";

interface SettingsState {
  settings: Settings | null;
  isLoading: boolean;
  isUpdating: Record<string, boolean>;
  audioDevices: AudioDevice[];
  outputDevices: AudioDevice[];
}

interface UseSettingsReturn {
  // State
  settings: Settings | null;
  isLoading: boolean;
  isUpdating: (key: string) => boolean;
  audioDevices: AudioDevice[];
  outputDevices: AudioDevice[];

  // Actions
  updateSetting: <K extends keyof Settings>(
    key: K,
    value: Settings[K],
  ) => Promise<void>;
  resetSetting: (key: keyof Settings) => Promise<void>;
  refreshSettings: () => Promise<void>;
  refreshAudioDevices: () => Promise<void>;
  refreshOutputDevices: () => Promise<void>;

  // Binding-specific actions
  updateBinding: (id: string, binding: string) => Promise<void>;
  resetBinding: (id: string) => Promise<void>;

  // Convenience getters
  getSetting: <K extends keyof Settings>(key: K) => Settings[K] | undefined;
}

export const useSettings = (): UseSettingsReturn => {
  const [state, setState] = useState<SettingsState>({
    settings: null,
    isLoading: true,
    isUpdating: {},
    audioDevices: [],
    outputDevices: [],
  });

  // Save settings to the store
  const saveSettings = useCallback(async (settingsToSave: Settings) => {
    try {
      const { load } = await import("@tauri-apps/plugin-store");
      const store = await load("settings_store.json", { autoSave: false });
      await store.set("settings", settingsToSave);
      await store.save();
      console.log("Settings saved successfully.");
    } catch (error) {
      console.error("Failed to save settings:", error);
    }
  }, []);

  // Load settings from store
  const loadSettings = useCallback(async () => {
    try {
      const { load } = await import("@tauri-apps/plugin-store");
      const store = await load("settings_store.json", { autoSave: false });
      const settings = (await store.get("settings")) as Settings;

      setState((prev) => ({
        ...prev,
        settings,
        isLoading: false,
      }));
    } catch (error) {
      console.error("Failed to load settings:", error);
      setState((prev) => ({
        ...prev,
        isLoading: false,
      }));
    }
  }, []);

  // Load audio devices
  const loadAudioDevices = useCallback(async () => {
    try {
      const devices: AudioDevice[] = await invoke("get_available_microphones");

      // Always ensure "Default" is available as the first option
      const devicesWithDefault = [
        { index: "default", name: "Default", is_default: true },
        ...devices.filter((d) => d.name !== "Default" && d.name !== "default"),
      ];

      setState((prev) => ({
        ...prev,
        audioDevices: devicesWithDefault,
      }));
    } catch (error) {
      console.error("Failed to load audio devices:", error);
      setState((prev) => ({
        ...prev,
        audioDevices: [{ index: "default", name: "Default", is_default: true }],
      }));
    }
  }, []);

  // Load output devices
  const loadOutputDevices = useCallback(async () => {
    try {
      const devices: AudioDevice[] = await invoke(
        "get_available_output_devices",
      );

      // Always ensure "Default" is available as the first option
      const devicesWithDefault = [
        { index: "default", name: "Default", is_default: true },
        ...devices.filter((d) => d.name !== "Default" && d.name !== "default"),
      ];

      setState((prev) => ({
        ...prev,
        outputDevices: devicesWithDefault,
      }));
    } catch (error) {
      console.error("Failed to load output devices:", error);
      setState((prev) => ({
        ...prev,
        outputDevices: [
          { index: "default", name: "Default", is_default: true },
        ],
      }));
    }
  }, []);

  // Update a specific setting
  const updateSetting = useCallback(
    async <K extends keyof Settings>(key: K, value: Settings[K]) => {
      const updateKey = String(key);

      // Set updating state
      setState((prev) => ({
        ...prev,
        isUpdating: { ...prev.isUpdating, [updateKey]: true },
      }));

      // Store original value for rollback
      const originalValue = state.settings?.[key];
      const originalSettings = state.settings;

      try {
        // Optimistic update
        const updatedSettings = state.settings
          ? { ...state.settings, [key]: value }
          : null;

        if (updatedSettings) {
          setState((prev) => ({
            ...prev,
            settings: updatedSettings,
          }));
        }

        // Invoke the appropriate backend method based on the setting
        switch (key) {
          case "always_on_microphone":
            await invoke("update_microphone_mode", { alwaysOn: value });
            break;
          case "audio_feedback":
            await invoke("change_audio_feedback_setting", { enabled: value });
            break;
          case "push_to_talk":
            await invoke("change_ptt_setting", { enabled: value });
            break;
          case "selected_microphone":
            // Map "Default" to "default" for backend compatibility
            const micDeviceName = value === "Default" ? "default" : value;
            await invoke("set_selected_microphone", {
              deviceName: micDeviceName,
            });
            break;
          case "selected_output_device":
            // Map "Default" to "default" for backend compatibility
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
          case "bindings":
            // Handle bindings separately - they use their own invoke methods
            break;
          case "selected_model":
            // Handle model selection if needed
            break;
          default:
            console.warn(`No handler for setting: ${String(key)}`);
        }

        console.log(`Setting ${String(key)} updated to:`, value);

        // Save the updated settings to the store
        if (state.settings) {
          const newSettings = { ...state.settings, [key]: value };
          await saveSettings(newSettings);
        }
      } catch (error) {
        console.error(`Failed to update setting ${String(key)}:`, error);

        // Rollback on error
        if (originalSettings) {
          setState((prev) => ({
            ...prev,
            settings: originalSettings,
          }));
        }
      } finally {
        // Clear updating state
        setState((prev) => ({
          ...prev,
          isUpdating: { ...prev.isUpdating, [updateKey]: false },
        }));
      }
    },
    [state.settings, saveSettings],
  );

  // Reset a setting to its default value
  const resetSetting = useCallback(
    async (key: keyof Settings) => {
      // Define default values
      const defaults: Partial<Settings> = {
        always_on_microphone: false,
        audio_feedback: true,
        push_to_talk: false,
        selected_microphone: "Default",
        selected_output_device: "Default",
        translate_to_english: false,
      };

      const defaultValue = defaults[key];
      if (defaultValue !== undefined) {
        await updateSetting(key, defaultValue as any);
      }
    },
    [updateSetting],
  );

  // Convenience getter
  const getSetting = useCallback(
    <K extends keyof Settings>(key: K): Settings[K] | undefined => {
      return state.settings?.[key];
    },
    [state.settings],
  );

  // Update a specific binding
  const updateBinding = useCallback(
    async (id: string, binding: string) => {
      const updateKey = `binding_${id}`;

      // Set updating state
      setState((prev) => ({
        ...prev,
        isUpdating: { ...prev.isUpdating, [updateKey]: true },
      }));

      // Store original binding for rollback
      const originalBinding = state.settings?.bindings?.[id]?.current_binding;
      const originalSettings = state.settings;

      try {
        // Optimistic update
        const updatedSettings = state.settings
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
          : null;

        if (updatedSettings) {
          setState((prev) => ({
            ...prev,
            settings: updatedSettings,
          }));
        }

        await invoke("change_binding", { id, binding });
        console.log(`Binding ${id} updated to: ${binding}`);

        // Save the updated settings to the store
        if (updatedSettings) {
          await saveSettings(updatedSettings);
        }
      } catch (error) {
        console.error(`Failed to update binding ${id}:`, error);

        // Rollback on error
        if (originalSettings) {
          setState((prev) => ({
            ...prev,
            settings: originalSettings,
          }));
        }
      } finally {
        // Clear updating state
        setState((prev) => ({
          ...prev,
          isUpdating: { ...prev.isUpdating, [updateKey]: false },
        }));
      }
    },
    [state.settings, saveSettings],
  );

  // Reset a specific binding
  const resetBinding = useCallback(
    async (id: string) => {
      const updateKey = `binding_${id}`;

      // Set updating state
      setState((prev) => ({
        ...prev,
        isUpdating: { ...prev.isUpdating, [updateKey]: true },
      }));

      try {
        const result = await invoke("reset_binding", { id });

        // Refresh settings to get the updated binding
        await loadSettings();

        console.log(`Binding ${id} reset to default`);
      } catch (error) {
        console.error(`Failed to reset binding ${id}:`, error);
      } finally {
        // Clear updating state
        setState((prev) => ({
          ...prev,
          isUpdating: { ...prev.isUpdating, [updateKey]: false },
        }));
      }
    },
    [loadSettings],
  );

  // Initialize
  useEffect(() => {
    loadSettings();
    loadAudioDevices();
    loadOutputDevices();
  }, [loadSettings, loadAudioDevices, loadOutputDevices]);

  return {
    settings: state.settings,
    isLoading: state.isLoading,
    isUpdating: (key: string) => state.isUpdating[key] || false,
    audioDevices: state.audioDevices,
    outputDevices: state.outputDevices,
    updateSetting,
    resetSetting,
    refreshSettings: loadSettings,
    refreshAudioDevices: loadAudioDevices,
    refreshOutputDevices: loadOutputDevices,
    updateBinding,
    resetBinding,
    getSetting,
  };
};
