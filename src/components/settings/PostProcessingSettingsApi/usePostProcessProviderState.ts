import { useCallback, useEffect, useMemo, useState } from "react";
import { useSettings } from "../../../hooks/useSettings";
import type { PostProcessProvider } from "../../../lib/types";
import type { ModelOption } from "./types";
import type { DropdownOption } from "../../ui/Dropdown";

const FALLBACK_PROVIDERS: PostProcessProvider[] = [
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

type PostProcessProviderState = {
  enabled: boolean;
  providerOptions: DropdownOption[];
  selectedProviderId: string;
  selectedProvider: PostProcessProvider | undefined;
  isCustomProvider: boolean;
  baseUrl: string;
  setBaseUrl: (value: string) => void;
  commitBaseUrl: () => void;
  isBaseUrlUpdating: boolean;
  apiKey: string;
  setApiKey: (value: string) => void;
  commitApiKey: () => void;
  isApiKeyUpdating: boolean;
  model: string;
  commitModel: () => void;
  modelOptions: ModelOption[];
  isModelUpdating: boolean;
  isFetchingModels: boolean;
  handleProviderSelect: (providerId: string) => void;
  handleModelSelect: (value: string) => void;
  handleModelCreate: (value: string) => void;
  handleRefreshModels: () => void;
};

export const usePostProcessProviderState = (): PostProcessProviderState => {
  const {
    settings,
    isUpdating,
    setPostProcessProvider,
    updatePostProcessBaseUrl,
    updatePostProcessApiKey,
    updatePostProcessModel,
    fetchPostProcessModels,
    postProcessModelOptions,
  } = useSettings();

  const enabled = settings?.post_process_enabled || false;

  const providers = useMemo(() => {
    const configured = settings?.post_process_providers || [];
    return configured.length > 0 ? configured : FALLBACK_PROVIDERS;
  }, [settings?.post_process_providers]);

  const selectedProviderId = useMemo(() => {
    if (settings?.post_process_provider_id) {
      return settings.post_process_provider_id;
    }
    return providers[0]?.id ?? "openai";
  }, [providers, settings?.post_process_provider_id]);

  const selectedProvider = useMemo(() => {
    return (
      providers.find((provider) => provider.id === selectedProviderId) ||
      providers[0]
    );
  }, [providers, selectedProviderId]);

  const storedBaseUrl = selectedProvider?.base_url ?? "";
  const storedApiKey =
    settings?.post_process_api_keys?.[selectedProviderId] ?? "";
  const storedModel = settings?.post_process_models?.[selectedProviderId] ?? "";

  const [baseUrl, setBaseUrl] = useState(storedBaseUrl);
  const [apiKey, setApiKey] = useState(storedApiKey);
  const [model, setModel] = useState(storedModel);

  useEffect(() => {
    setBaseUrl(storedBaseUrl);
  }, [storedBaseUrl]);

  useEffect(() => {
    setApiKey(storedApiKey);
  }, [storedApiKey]);

  useEffect(() => {
    setModel(storedModel);
  }, [storedModel]);

  const providerOptions = useMemo<DropdownOption[]>(() => {
    return providers.map((provider) => ({
      value: provider.id,
      label: provider.label,
    }));
  }, [providers]);

  const handleProviderSelect = useCallback(
    (providerId: string) => {
      if (providerId !== selectedProviderId) {
        void setPostProcessProvider(providerId);
      }
    },
    [selectedProviderId, setPostProcessProvider],
  );

  const commitBaseUrl = useCallback(() => {
    if (!selectedProvider || !selectedProvider.allow_base_url_edit) {
      return;
    }
    const trimmed = baseUrl.trim();
    if (trimmed && trimmed !== storedBaseUrl) {
      void updatePostProcessBaseUrl(selectedProvider.id, trimmed);
    }
  }, [
    baseUrl,
    selectedProvider,
    storedBaseUrl,
    updatePostProcessBaseUrl,
  ]);

  const commitApiKey = useCallback(() => {
    const trimmed = apiKey.trim();
    if (trimmed !== storedApiKey) {
      void updatePostProcessApiKey(selectedProviderId, trimmed);
    }
  }, [apiKey, storedApiKey, selectedProviderId, updatePostProcessApiKey]);

  const commitModel = useCallback(() => {
    const trimmed = model.trim();
    if (trimmed !== model) {
      setModel(trimmed);
    }
    if (trimmed !== storedModel) {
      void updatePostProcessModel(selectedProviderId, trimmed);
    }
  }, [model, storedModel, selectedProviderId, updatePostProcessModel]);

  const handleModelSelect = useCallback(
    (value: string) => {
      setModel(value);
      void updatePostProcessModel(selectedProviderId, value.trim());
    },
    [selectedProviderId, updatePostProcessModel],
  );

  const handleModelCreate = useCallback(
    (value: string) => {
      setModel(value);
      void updatePostProcessModel(selectedProviderId, value);
    },
    [selectedProviderId, updatePostProcessModel],
  );

  const handleRefreshModels = useCallback(() => {
    void fetchPostProcessModels(selectedProviderId);
  }, [fetchPostProcessModels, selectedProviderId]);

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

    for (const candidate of availableModelsRaw) {
      upsert(candidate);
    }

    upsert(storedModel);
    upsert(model);

    return options;
  }, [availableModelsRaw, storedModel, model]);

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

  useEffect(() => {
    if (isCustomProvider) {
      return;
    }

    const modelCount = postProcessModelOptions[selectedProviderId]?.length ?? 0;
    if (modelCount === 0 && !isFetchingModels) {
      void fetchPostProcessModels(selectedProviderId);
    }
  }, [
    isCustomProvider,
    isFetchingModels,
    fetchPostProcessModels,
    postProcessModelOptions,
    selectedProviderId,
  ]);

  return {
    enabled,
    providerOptions,
    selectedProviderId,
    selectedProvider,
    isCustomProvider,
    baseUrl,
    setBaseUrl,
    commitBaseUrl,
    isBaseUrlUpdating,
    apiKey,
    setApiKey,
    commitApiKey,
    isApiKeyUpdating,
    model,
    commitModel,
    modelOptions,
    isModelUpdating,
    isFetchingModels,
    handleProviderSelect,
    handleModelSelect,
    handleModelCreate,
    handleRefreshModels,
  };
};
