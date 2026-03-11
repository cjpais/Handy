import { useCallback, useMemo } from "react";
import { type PostProcessProvider } from "@/bindings";
import type { ModelOption } from "./types";
import type { DropdownOption } from "../../ui/Dropdown";
import { useSettings } from "../../../hooks/useSettings";

type RewriteProviderState = {
  providerOptions: DropdownOption[];
  selectedProviderId: string;
  selectedProvider: PostProcessProvider | undefined;
  isCustomProvider: boolean;
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

export const useRewriteProviderState = (): RewriteProviderState => {
  const {
    settings,
    isUpdating,
    setRewriteProvider,
    updateRewriteBaseUrl,
    updateRewriteApiKey,
    updateRewriteModel,
    fetchRewriteModels,
    postProcessModelOptions,
  } = useSettings();

  const providers = settings?.rewrite_providers || [];

  const selectedProviderId = useMemo(() => {
    return settings?.rewrite_provider_id || providers[0]?.id || "openai";
  }, [providers, settings?.rewrite_provider_id]);

  const selectedProvider = useMemo(() => {
    return (
      providers.find((provider) => provider.id === selectedProviderId) ||
      providers[0]
    );
  }, [providers, selectedProviderId]);

  const baseUrl = selectedProvider?.base_url ?? "";
  const apiKey = settings?.rewrite_api_keys?.[selectedProviderId] ?? "";
  const model = settings?.rewrite_models?.[selectedProviderId] ?? "";

  const providerOptions = useMemo<DropdownOption[]>(() => {
    return providers.map((provider) => ({
      value: provider.id,
      label: provider.label,
    }));
  }, [providers]);

  const handleProviderSelect = useCallback(
    async (providerId: string) => {
      if (providerId === selectedProviderId) return;
      await setRewriteProvider(providerId);

      const provider = providers.find((p) => p.id === providerId);
      const apiKey = settings?.rewrite_api_keys?.[providerId] ?? "";
      const hasBaseUrl = (provider?.base_url ?? "").trim() !== "";
      const hasApiKey = apiKey.trim() !== "";

      if (provider?.id === "custom" ? hasBaseUrl : hasApiKey) {
        void fetchRewriteModels(providerId);
      }
    },
    [
      selectedProviderId,
      setRewriteProvider,
      providers,
      settings,
      fetchRewriteModels,
    ],
  );

  const handleBaseUrlChange = useCallback(
    (value: string) => {
      if (!selectedProvider || selectedProvider.id !== "custom") {
        return;
      }
      const trimmed = value.trim();
      if (trimmed && trimmed !== baseUrl) {
        void updateRewriteBaseUrl(selectedProvider.id, trimmed);
      }
    },
    [selectedProvider, baseUrl, updateRewriteBaseUrl],
  );

  const handleApiKeyChange = useCallback(
    (value: string) => {
      const trimmed = value.trim();
      if (trimmed !== apiKey) {
        void updateRewriteApiKey(selectedProviderId, trimmed);
      }
    },
    [apiKey, selectedProviderId, updateRewriteApiKey],
  );

  const handleModelChange = useCallback(
    (value: string) => {
      const trimmed = value.trim();
      if (trimmed !== model) {
        void updateRewriteModel(selectedProviderId, trimmed);
      }
    },
    [model, selectedProviderId, updateRewriteModel],
  );

  const handleModelSelect = useCallback(
    (value: string) => {
      void updateRewriteModel(selectedProviderId, value.trim());
    },
    [selectedProviderId, updateRewriteModel],
  );

  const handleModelCreate = useCallback(
    (value: string) => {
      void updateRewriteModel(selectedProviderId, value);
    },
    [selectedProviderId, updateRewriteModel],
  );

  const handleRefreshModels = useCallback(() => {
    void fetchRewriteModels(selectedProviderId);
  }, [fetchRewriteModels, selectedProviderId]);

  const shouldDisableModelCreate =
    !selectedProvider ||
    (selectedProvider.id !== "custom" && apiKey.trim() === "");

  const availableModelsRaw =
    postProcessModelOptions[`rewrite:${selectedProviderId}`] || [];

  const modelOptions = useMemo<ModelOption[]>(() => {
    const seen = new Set<string>();
    const options: ModelOption[] = [];

    const upsert = (value: string | null | undefined) => {
      const trimmed = value?.trim();
      if (!trimmed || seen.has(trimmed)) return;
      if (shouldDisableModelCreate && !availableModelsRaw.includes(trimmed))
        return;
      seen.add(trimmed);
      options.push({ value: trimmed, label: trimmed });
    };

    for (const candidate of availableModelsRaw) {
      upsert(candidate);
    }

    upsert(model);

    return options;
  }, [availableModelsRaw, model, shouldDisableModelCreate]);

  const isBaseUrlUpdating = isUpdating(
    `rewrite_base_url:${selectedProviderId}`,
  );
  const isApiKeyUpdating = isUpdating(`rewrite_api_key:${selectedProviderId}`);
  const isModelUpdating = isUpdating(`rewrite_model:${selectedProviderId}`);
  const isFetchingModels = isUpdating(
    `rewrite_models_fetch:${selectedProviderId}`,
  );

  const isCustomProvider = selectedProvider?.id === "custom";

  return {
    providerOptions,
    selectedProviderId,
    selectedProvider,
    isCustomProvider,
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
