import type { AppSettings } from "../../../src/bindings";
import {
  MOCK_AUDIO_DEVICES,
  MOCK_OUTPUT_DEVICES,
  MOCK_SETTINGS,
} from "./data";

let settingsState: AppSettings = { ...MOCK_SETTINGS };

const updateSettingsState = (partial: Partial<AppSettings>) => {
  settingsState = { ...settingsState, ...partial };
};

export const useSettings = () => {
  const settings = settingsState;

  const updateSetting = async <K extends keyof AppSettings>(
    key: K,
    value: AppSettings[K],
  ) => {
    updateSettingsState({ [key]: value } as Partial<AppSettings>);
  };

  const resetSetting = async (key: keyof AppSettings) => {
    updateSettingsState({ [key]: MOCK_SETTINGS[key] } as Partial<AppSettings>);
  };

  const updateBinding = async (id: string, binding: string) => {
    const nextBindings = {
      ...(settingsState.bindings || {}),
      [id]: {
        ...(settingsState.bindings?.[id] ?? {
          id,
          name: id,
          description: "",
          default_binding: binding,
          current_binding: binding,
        }),
        current_binding: binding,
      },
    };
    updateSettingsState({ bindings: nextBindings });
  };

  const resetBinding = async (id: string) => {
    const binding = settingsState.bindings?.[id];
    if (!binding) return;
    updateBinding(id, binding.default_binding);
  };

  const setPostProcessProvider = async (providerId: string) => {
    updateSettingsState({ post_process_provider_id: providerId });
  };

  const updatePostProcessBaseUrl = async (
    providerId: string,
    baseUrl: string,
  ) => {
    const providers = settingsState.post_process_providers || [];
    const nextProviders = providers.map((provider) =>
      provider.id === providerId ? { ...provider, base_url: baseUrl } : provider,
    );
    updateSettingsState({ post_process_providers: nextProviders });
  };

  const updatePostProcessApiKey = async (
    providerId: string,
    apiKey: string,
  ) => {
    updateSettingsState({
      post_process_api_keys: {
        ...(settingsState.post_process_api_keys || {}),
        [providerId]: apiKey,
      },
    });
  };

  const updatePostProcessModel = async (providerId: string, model: string) => {
    updateSettingsState({
      post_process_models: {
        ...(settingsState.post_process_models || {}),
        [providerId]: model,
      },
    });
  };

  const postProcessModelOptions: Record<string, string[]> = {
    openai: ["gpt-4o-mini", "gpt-4o"],
    custom: ["custom-llm", "local-llm"],
  };

  return {
    settings,
    isLoading: false,
    isUpdating: () => false,
    audioDevices: MOCK_AUDIO_DEVICES,
    outputDevices: MOCK_OUTPUT_DEVICES,
    audioFeedbackEnabled: settings.audio_feedback || false,
    postProcessModelOptions,
    updateSetting,
    resetSetting,
    refreshSettings: async () => {},
    refreshAudioDevices: async () => {},
    refreshOutputDevices: async () => {},
    updateBinding,
    resetBinding,
    getSetting: <K extends keyof AppSettings>(key: K) => settings[key],
    setPostProcessProvider,
    updatePostProcessBaseUrl,
    updatePostProcessApiKey,
    updatePostProcessModel,
    fetchPostProcessModels: async (providerId: string) =>
      postProcessModelOptions[providerId] || [],
  };
};
