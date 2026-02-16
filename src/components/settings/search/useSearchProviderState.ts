import { useCallback, useMemo, useState } from "react";
import { useSettings } from "../../../hooks/useSettings";
import { commands, type PostProcessProvider } from "@/bindings";
import type { ModelOption } from "../PostProcessingSettingsApi/types";
import type { DropdownOption } from "../../ui/Dropdown";

type SearchProviderState = {
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
};

const APPLE_PROVIDER_ID = "apple_intelligence";

export const useSearchProviderState = (): SearchProviderState => {
  const {
    settings,
    isUpdating,
    setSearchProvider,
    updateSearchBaseUrl,
    updateSearchApiKey,
    updateSearchModel,
    fetchSearchModels,
    searchModelOptions,
  } = useSettings();

  const providers = settings?.search_providers || [];

  const selectedProviderId = useMemo(() => {
    return settings?.search_provider_id || providers[0]?.id || "openai";
  }, [providers, settings?.search_provider_id]);

  const selectedProvider = useMemo(() => {
    return (
      providers.find((provider) => provider.id === selectedProviderId) ||
      providers[0]
    );
  }, [providers, selectedProviderId]);

  const isAppleProvider = selectedProvider?.id === APPLE_PROVIDER_ID;
  const [appleIntelligenceUnavailable, setAppleIntelligenceUnavailable] =
    useState(false);

  const baseUrl = selectedProvider?.base_url ?? "";
  const apiKey = settings?.search_api_keys?.[selectedProviderId] ?? "";
  const model = settings?.search_models?.[selectedProviderId] ?? "";

  const providerOptions = useMemo<DropdownOption[]>(() => {
    return providers.map((provider) => ({
      value: provider.id,
      label: provider.label,
    }));
  }, [providers]);

  const handleProviderSelect = useCallback(
    async (providerId: string) => {
      setAppleIntelligenceUnavailable(false);

      if (providerId === selectedProviderId) return;

      if (providerId === APPLE_PROVIDER_ID) {
        const available = await commands.checkAppleIntelligenceAvailable();
        if (!available) {
          setAppleIntelligenceUnavailable(true);
        }
      }

      void setSearchProvider(providerId);
    },
    [selectedProviderId, setSearchProvider],
  );

  const handleBaseUrlChange = useCallback(
    (value: string) => {
      if (!selectedProvider || selectedProvider.id !== "custom") {
        return;
      }
      const trimmed = value.trim();
      if (trimmed && trimmed !== baseUrl) {
        void updateSearchBaseUrl(selectedProvider.id, trimmed);
      }
    },
    [selectedProvider, baseUrl, updateSearchBaseUrl],
  );

  const handleApiKeyChange = useCallback(
    (value: string) => {
      const trimmed = value.trim();
      if (trimmed !== apiKey) {
        void updateSearchApiKey(selectedProviderId, trimmed);
      }
    },
    [apiKey, selectedProviderId, updateSearchApiKey],
  );

  const handleModelChange = useCallback(
    (value: string) => {
      const trimmed = value.trim();
      if (trimmed !== model) {
        void updateSearchModel(selectedProviderId, trimmed);
      }
    },
    [model, selectedProviderId, updateSearchModel],
  );

  const handleModelSelect = useCallback(
    (value: string) => {
      void updateSearchModel(selectedProviderId, value.trim());
    },
    [selectedProviderId, updateSearchModel],
  );

  const handleModelCreate = useCallback(
    (value: string) => {
      void updateSearchModel(selectedProviderId, value);
    },
    [selectedProviderId, updateSearchModel],
  );

  const handleRefreshModels = useCallback(() => {
    if (isAppleProvider) return;
    void fetchSearchModels(selectedProviderId);
  }, [fetchSearchModels, isAppleProvider, selectedProviderId]);

  const availableModelsRaw = searchModelOptions[selectedProviderId] || [];

  const modelOptions = useMemo<ModelOption[]>(() => {
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

    upsert(model);

    return options;
  }, [availableModelsRaw, model]);

  const isBaseUrlUpdating = isUpdating(`search_base_url:${selectedProviderId}`);
  const isApiKeyUpdating = isUpdating(`search_api_key:${selectedProviderId}`);
  const isModelUpdating = isUpdating(`search_model:${selectedProviderId}`);
  const isFetchingModels = isUpdating(
    `search_models_fetch:${selectedProviderId}`,
  );

  const isCustomProvider = selectedProvider?.id === "custom";

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
  };
};
