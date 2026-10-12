import { useCallback, useMemo, useState } from "react";
import { useSettings } from "../../../hooks/useSettings";
import { commands, type PostProcessProvider } from "@/bindings";
import type { ModelOption } from "./types";
import type { DropdownOption } from "../../ui/Dropdown";

type PostProcessProviderState = {
  providerOptions: DropdownOption[];
  selectedProviderId: string;
  selectedProvider: PostProcessProvider | undefined;
  isCustomProvider: boolean;
  isAppleProvider: boolean;
  appleIntelligenceUnavailable: boolean;
  baseUrl: string;
  handleBaseUrlChange: (value: string) => void;
  isBaseUrlUpdating: boolean;
  apiKey: string;
  handleApiKeyChange: (value: string) => void;
  isApiKeyUpdating: boolean;
  model: string;
  handleModelChange: (value: string) => void;
  modelOptions: ModelOption[];
  isModelUpdating: boolean;
  isFetchingModels: boolean;
  handleProviderSelect: (providerId: string) => void;
  handleModelSelect: (value: string) => void;
  handleModelCreate: (value: string) => void;
  handleRefreshModels: () => void;
  // Fallback (secondary) model, used when the primary model fails.
  fallbackEnabled: boolean;
  handleFallbackEnabledChange: (enabled: boolean) => void;
  isFallbackEnabledUpdating: boolean;
  fallbackUseSeparateKey: boolean;
  handleFallbackUseSeparateKeyChange: (enabled: boolean) => void;
  isFallbackSeparateKeyUpdating: boolean;
  fallbackModel: string;
  handleFallbackModelChange: (value: string) => void;
  handleFallbackModelSelect: (value: string) => void;
  handleFallbackModelCreate: (value: string) => void;
  isFallbackModelUpdating: boolean;
  fallbackModelOptions: ModelOption[];
  fallbackApiKey: string;
  handleFallbackApiKeyChange: (value: string) => void;
  isFallbackApiKeyUpdating: boolean;
};

const APPLE_PROVIDER_ID = "apple_intelligence";

export const usePostProcessProviderState = (): PostProcessProviderState => {
  const {
    settings,
    isUpdating,
    updateSetting,
    setPostProcessProvider,
    updatePostProcessBaseUrl,
    updatePostProcessApiKey,
    updatePostProcessModel,
    updatePostProcessFallbackModel,
    updatePostProcessFallbackApiKey,
    fetchPostProcessModels,
    postProcessModelOptions,
  } = useSettings();

  // Settings are guaranteed to have providers after migration
  const providers = settings?.post_process_providers || [];

  const selectedProviderId = useMemo(() => {
    return settings?.post_process_provider_id || providers[0]?.id || "openai";
  }, [providers, settings?.post_process_provider_id]);

  const selectedProvider = useMemo(() => {
    return (
      providers.find((provider) => provider.id === selectedProviderId) ||
      providers[0]
    );
  }, [providers, selectedProviderId]);

  const isAppleProvider = selectedProvider?.id === APPLE_PROVIDER_ID;
  const [appleIntelligenceUnavailable, setAppleIntelligenceUnavailable] =
    useState(false);

  // Use settings directly as single source of truth
  const baseUrl = selectedProvider?.base_url ?? "";
  const apiKey = settings?.post_process_api_keys?.[selectedProviderId] ?? "";
  const model = settings?.post_process_models?.[selectedProviderId] ?? "";

  const providerOptions = useMemo<DropdownOption[]>(() => {
    return providers.map((provider) => ({
      value: provider.id,
      label: provider.label,
    }));
  }, [providers]);

  const handleProviderSelect = useCallback(
    async (providerId: string) => {
      // Clear error state on any selection attempt (allows dismissing the error)
      setAppleIntelligenceUnavailable(false);

      if (providerId === selectedProviderId) return;

      // Check Apple Intelligence availability before selecting
      if (providerId === APPLE_PROVIDER_ID) {
        const available = await commands.checkAppleIntelligenceAvailable();
        if (!available) {
          setAppleIntelligenceUnavailable(true);
          // Don't return - still set the provider so dropdown shows the selection
          // The backend gracefully handles unavailable Apple Intelligence
        }
      }

      await setPostProcessProvider(providerId);

      // Auto-fetch available models for the new provider so the model dropdown
      // reflects what's actually valid. Without this, a stale model value from
      // a previous provider/base_url can persist and silently 404 at runtime.
      // Skip when the provider isn't configured yet (no API key / empty base URL)
      // to avoid unnecessary backend errors.
      if (providerId !== APPLE_PROVIDER_ID) {
        const provider = providers.find((p) => p.id === providerId);
        const apiKey = settings?.post_process_api_keys?.[providerId] ?? "";
        const hasBaseUrl = (provider?.base_url ?? "").trim() !== "";
        const hasApiKey = apiKey.trim() !== "";

        if (provider?.id === "custom" ? hasBaseUrl : hasApiKey) {
          void fetchPostProcessModels(providerId);
        }
      }
    },
    [
      selectedProviderId,
      setPostProcessProvider,
      fetchPostProcessModels,
      providers,
      settings,
    ],
  );

  const handleBaseUrlChange = useCallback(
    (value: string) => {
      if (!selectedProvider || selectedProvider.id !== "custom") {
        return;
      }
      const trimmed = value.trim();
      if (trimmed && trimmed !== baseUrl) {
        void updatePostProcessBaseUrl(selectedProvider.id, trimmed);
      }
    },
    [selectedProvider, baseUrl, updatePostProcessBaseUrl],
  );

  const handleApiKeyChange = useCallback(
    (value: string) => {
      const trimmed = value.trim();
      if (trimmed !== apiKey) {
        void updatePostProcessApiKey(selectedProviderId, trimmed);
      }
    },
    [apiKey, selectedProviderId, updatePostProcessApiKey],
  );

  const handleModelChange = useCallback(
    (value: string) => {
      const trimmed = value.trim();
      if (trimmed !== model) {
        void updatePostProcessModel(selectedProviderId, trimmed);
      }
    },
    [model, selectedProviderId, updatePostProcessModel],
  );

  const handleModelSelect = useCallback(
    (value: string) => {
      void updatePostProcessModel(selectedProviderId, value.trim());
    },
    [selectedProviderId, updatePostProcessModel],
  );

  const handleModelCreate = useCallback(
    (value: string) => {
      void updatePostProcessModel(selectedProviderId, value);
    },
    [selectedProviderId, updatePostProcessModel],
  );

  const handleRefreshModels = useCallback(() => {
    if (isAppleProvider) return;
    void fetchPostProcessModels(selectedProviderId);
  }, [fetchPostProcessModels, isAppleProvider, selectedProviderId]);

  const availableModelsRaw = postProcessModelOptions[selectedProviderId] || [];

  const modelOptions = useMemo<ModelOption[]>(() => {
    const seen = new Set<string>();
    const options: ModelOption[] = [];

    const upsert = (value: string | null | undefined) => {
      const trimmed = value?.trim();
      if (!trimmed || seen.has(trimmed)) return;
      seen.add(trimmed);
      options.push({ value: trimmed, label: trimmed });
    };

    // Add available models from API
    for (const candidate of availableModelsRaw) {
      upsert(candidate);
    }

    // Ensure current model is in the list
    upsert(model);

    return options;
  }, [availableModelsRaw, model]);

  const isBaseUrlUpdating = isUpdating(
    `post_process_base_url:${selectedProviderId}`,
  );
  const isApiKeyUpdating = isUpdating(
    `post_process_api_key:${selectedProviderId}`,
  );
  const isModelUpdating = isUpdating(
    `post_process_model:${selectedProviderId}`,
  );
  const isFetchingModels = isUpdating(
    `post_process_models_fetch:${selectedProviderId}`,
  );

  const isCustomProvider = selectedProvider?.id === "custom";

  // No automatic fetching - user must click refresh button

  const fallbackEnabled = settings?.post_process_fallback_enabled ?? false;
  const fallbackUseSeparateKey =
    settings?.post_process_fallback_use_separate_key ?? false;
  const fallbackModel =
    settings?.post_process_fallback_models?.[selectedProviderId] ?? "";
  const fallbackApiKey =
    settings?.post_process_fallback_api_keys?.[selectedProviderId] ?? "";

  const handleFallbackEnabledChange = useCallback(
    (enabled: boolean) => {
      void updateSetting("post_process_fallback_enabled", enabled);
    },
    [updateSetting],
  );

  const handleFallbackUseSeparateKeyChange = useCallback(
    (enabled: boolean) => {
      void updateSetting("post_process_fallback_use_separate_key", enabled);
    },
    [updateSetting],
  );

  const handleFallbackModelChange = useCallback(
    (value: string) => {
      const trimmed = value.trim();
      if (trimmed !== fallbackModel) {
        void updatePostProcessFallbackModel(selectedProviderId, trimmed);
      }
    },
    [fallbackModel, selectedProviderId, updatePostProcessFallbackModel],
  );

  const handleFallbackModelSelect = useCallback(
    (value: string) => {
      void updatePostProcessFallbackModel(selectedProviderId, value.trim());
    },
    [selectedProviderId, updatePostProcessFallbackModel],
  );

  const handleFallbackModelCreate = useCallback(
    (value: string) => {
      void updatePostProcessFallbackModel(selectedProviderId, value);
    },
    [selectedProviderId, updatePostProcessFallbackModel],
  );

  const handleFallbackApiKeyChange = useCallback(
    (value: string) => {
      const trimmed = value.trim();
      if (trimmed !== fallbackApiKey) {
        void updatePostProcessFallbackApiKey(selectedProviderId, trimmed);
      }
    },
    [fallbackApiKey, selectedProviderId, updatePostProcessFallbackApiKey],
  );

  // The fallback draws from the same provider list as the primary model, so it
  // can reuse the fetched options and still offer the configured value.
  const fallbackModelOptions = useMemo<ModelOption[]>(() => {
    const seen = new Set<string>();
    const options: ModelOption[] = [];

    const upsert = (value: string | null | undefined) => {
      const trimmed = value?.trim();
      if (!trimmed || seen.has(trimmed)) return;
      seen.add(trimmed);
      options.push({ value: trimmed, label: trimmed });
    };

    for (const candidate of availableModelsRaw) {
      upsert(candidate);
    }
    upsert(fallbackModel);

    return options;
  }, [availableModelsRaw, fallbackModel]);

  const isFallbackEnabledUpdating = isUpdating(
    "post_process_fallback_enabled",
  );
  const isFallbackSeparateKeyUpdating = isUpdating(
    "post_process_fallback_use_separate_key",
  );
  const isFallbackModelUpdating = isUpdating(
    `post_process_fallback_model:${selectedProviderId}`,
  );
  const isFallbackApiKeyUpdating = isUpdating(
    `post_process_fallback_api_key:${selectedProviderId}`,
  );

  return {
    providerOptions,
    selectedProviderId,
    selectedProvider,
    isCustomProvider,
    isAppleProvider,
    appleIntelligenceUnavailable,
    baseUrl,
    handleBaseUrlChange,
    isBaseUrlUpdating,
    apiKey,
    handleApiKeyChange,
    isApiKeyUpdating,
    model,
    handleModelChange,
    modelOptions,
    isModelUpdating,
    isFetchingModels,
    handleProviderSelect,
    handleModelSelect,
    handleModelCreate,
    handleRefreshModels,
    fallbackEnabled,
    handleFallbackEnabledChange,
    isFallbackEnabledUpdating,
    fallbackUseSeparateKey,
    handleFallbackUseSeparateKeyChange,
    isFallbackSeparateKeyUpdating,
    fallbackModel,
    handleFallbackModelChange,
    handleFallbackModelSelect,
    handleFallbackModelCreate,
    isFallbackModelUpdating,
    fallbackModelOptions,
    fallbackApiKey,
    handleFallbackApiKeyChange,
    isFallbackApiKeyUpdating,
  };
};
