import { create } from "zustand";
import { subscribeWithSelector } from "zustand/middleware";
import { listen } from "@tauri-apps/api/event";
import type { AppSettings as Settings, AudioDevice } from "@/bindings";
import { commands } from "@/bindings";

interface SettingsStore {
  settings: Settings | null;
  defaultSettings: Settings | null;
  isLoading: boolean;
  isUpdating: Record<string, boolean>;
  audioDevices: AudioDevice[];

  // Actions
  initialize: () => Promise<void>;
  updateSetting: <K extends keyof Settings>(
    key: K,
    value: Settings[K],
  ) => Promise<void>;
  resetSetting: (key: keyof Settings) => Promise<void>;
  refreshSettings: () => Promise<void>;
  refreshAudioDevices: () => Promise<void>;
  updateBinding: (id: string, binding: string) => Promise<void>;
  resetBinding: (id: string) => Promise<void>;
  getSetting: <K extends keyof Settings>(key: K) => Settings[K] | undefined;
  isUpdatingKey: (key: string) => boolean;
}

const DEFAULT_AUDIO_DEVICE: AudioDevice = {
  index: "default",
  name: "Default",
  is_default: true,
};

// How each setting this window edits reaches the backend.
const settingUpdaters: {
  [K in keyof Settings]?: (value: Settings[K]) => Promise<unknown>;
} = {
  start_hidden: (value) => commands.changeStartHiddenSetting(value as boolean),
  autostart_enabled: (value) =>
    commands.changeAutostartSetting(value as boolean),
  selected_microphone: (value) =>
    commands.setSelectedMicrophone(
      (value as string) === "Default" || value === null
        ? "default"
        : (value as string),
    ),
  overlay_position: (value) =>
    commands.changeOverlayPositionSetting(value as string),
  overlay_style: (value) => commands.changeOverlayStyleSetting(value as string),
};

export const useSettingsStore = create<SettingsStore>()(
  subscribeWithSelector((set, get) => ({
    settings: null,
    defaultSettings: null,
    isLoading: true,
    isUpdating: {},
    audioDevices: [],

    getSetting: (key) => get().settings?.[key],
    isUpdatingKey: (key) => get().isUpdating[key] || false,

    // Load settings from store
    refreshSettings: async () => {
      try {
        const result = await commands.getAppSettings();
        if (result.status === "ok") {
          const settings = result.data;
          set({
            settings: {
              ...settings,
              selected_microphone: settings.selected_microphone ?? "Default",
            },
            isLoading: false,
          });
        } else {
          console.error("Failed to load settings:", result.error);
          set({ isLoading: false });
        }
      } catch (error) {
        console.error("Failed to load settings:", error);
        set({ isLoading: false });
      }
    },

    // Load audio devices
    refreshAudioDevices: async () => {
      try {
        const result = await commands.getAvailableMicrophones();
        if (result.status === "ok") {
          set({
            audioDevices: [
              DEFAULT_AUDIO_DEVICE,
              ...result.data.filter(
                (d) => d.name !== "Default" && d.name !== "default",
              ),
            ],
          });
        } else {
          set({ audioDevices: [DEFAULT_AUDIO_DEVICE] });
        }
      } catch (error) {
        console.error("Failed to load audio devices:", error);
        set({ audioDevices: [DEFAULT_AUDIO_DEVICE] });
      }
    },

    // Update a specific setting
    updateSetting: async <K extends keyof Settings>(
      key: K,
      value: Settings[K],
    ) => {
      const { settings } = get();
      const updateKey = String(key);
      const originalValue = settings?.[key];
      const setUpdating = (updating: boolean) =>
        set((state) => ({
          isUpdating: { ...state.isUpdating, [updateKey]: updating },
        }));

      setUpdating(true);

      try {
        set((state) => ({
          settings: state.settings ? { ...state.settings, [key]: value } : null,
        }));

        const updater = settingUpdaters[key];
        if (updater) {
          await updater(value);
        } else {
          console.warn(`No handler for setting: ${updateKey}`);
        }
      } catch (error) {
        console.error(`Failed to update setting ${updateKey}:`, error);
        if (settings) {
          set({ settings: { ...settings, [key]: originalValue } });
        }
      } finally {
        setUpdating(false);
      }
    },

    // Reset a setting to its default value
    resetSetting: async (key) => {
      const { defaultSettings } = get();
      if (defaultSettings) {
        const defaultValue = defaultSettings[key];
        if (defaultValue !== undefined) {
          await get().updateSetting(key, defaultValue as never);
        }
      }
    },

    // Update a specific binding
    updateBinding: async (id, binding) => {
      const { settings } = get();
      const updateKey = `binding_${id}`;
      const originalBinding = settings?.bindings?.[id]?.current_binding;
      const setUpdating = (updating: boolean) =>
        set((state) => ({
          isUpdating: { ...state.isUpdating, [updateKey]: updating },
        }));
      const setBinding = (value: string) =>
        set((state) => ({
          settings: state.settings
            ? {
                ...state.settings,
                bindings: {
                  ...state.settings.bindings,
                  [id]: {
                    ...state.settings.bindings?.[id]!,
                    current_binding: value,
                  },
                },
              }
            : null,
        }));

      setUpdating(true);

      try {
        // Optimistic update
        setBinding(binding);

        const result = await commands.changeBinding(id, binding);
        if (result.status === "error") {
          throw new Error(result.error);
        }
        if (!result.data.success) {
          throw new Error(result.data.error || "Failed to update binding");
        }
      } catch (error) {
        console.error(`Failed to update binding ${id}:`, error);

        // Rollback on error
        if (originalBinding && get().settings) {
          setBinding(originalBinding);
        }

        // Re-throw to let the caller know it failed
        throw error;
      } finally {
        setUpdating(false);
      }
    },

    // Reset a specific binding
    resetBinding: async (id) => {
      const updateKey = `binding_${id}`;
      const setUpdating = (updating: boolean) =>
        set((state) => ({
          isUpdating: { ...state.isUpdating, [updateKey]: updating },
        }));

      setUpdating(true);

      try {
        await commands.resetBinding(id);
        await get().refreshSettings();
      } catch (error) {
        console.error(`Failed to reset binding ${id}:`, error);
      } finally {
        setUpdating(false);
      }
    },

    // Initialize everything
    initialize: async () => {
      const { refreshSettings } = get();

      // Note: Audio devices are NOT refreshed here. App.tsx refreshes them
      // once the permissions are in place. This avoids triggering permission
      // dialogs on macOS before the user is ready.
      const defaults = await commands.getDefaultSettings();
      if (defaults.status === "ok") {
        set({ defaultSettings: defaults.data });
      }
      await refreshSettings();

      // Re-fetch settings when the backend changes them. The backend is the
      // source of truth.
      listen<{ setting?: string }>("settings-changed", (event) => {
        get().refreshSettings();
        if (event.payload.setting === "selected_microphone") {
          get().refreshAudioDevices();
        }
      });
    },
  })),
);
