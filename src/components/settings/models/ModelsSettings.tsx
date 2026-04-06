import React, { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ask } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { ChevronDown, Globe } from "lucide-react";
import type { ModelCardStatus } from "@/components/onboarding";
import { ModelCard } from "@/components/onboarding";
import { commands, type ModelInfo } from "@/bindings";
import { useModelStore } from "@/stores/modelStore";
import { LANGUAGES } from "@/lib/constants/languages.ts";
import { useSettings } from "../../../hooks/useSettings";
import { ExternalTranscriptionModelCard } from "./ExternalTranscriptionModelCard";
import {
  ELEVENLABS_TRANSCRIPTION_PROVIDER_ID,
  getElevenLabsModelProfile,
  hasTranscriptionProviderApiKey,
} from "@/lib/utils/externalTranscriptionModel";

// check if model supports a language based on its supported_languages list
const modelSupportsLanguage = (model: ModelInfo, langCode: string): boolean => {
  return model.supported_languages.includes(langCode);
};

export const ModelsSettings: React.FC = () => {
  const { t } = useTranslation();
  const [switchingModelId, setSwitchingModelId] = useState<string | null>(null);
  const [languageFilter, setLanguageFilter] = useState("all");
  const [languageDropdownOpen, setLanguageDropdownOpen] = useState(false);
  const [languageSearch, setLanguageSearch] = useState("");
  const [elevenLabsApiKeySaving, setElevenLabsApiKeySaving] = useState(false);
  const [elevenLabsApiKeyError, setElevenLabsApiKeyError] = useState<
    string | null
  >(null);
  const languageDropdownRef = useRef<HTMLDivElement>(null);
  const languageSearchInputRef = useRef<HTMLInputElement>(null);
  const { getSetting, updateSetting, refreshSettings } = useSettings();
  const {
    models,
    currentModel,
    downloadingModels,
    downloadProgress,
    downloadStats,
    verifyingModels,
    extractingModels,
    loading,
    downloadModel,
    cancelDownload,
    selectModel,
    deleteModel,
  } = useModelStore();
  const transcriptionProviderId =
    getSetting("transcription_provider_id") || "local";
  const transcriptionApiKeys = getSetting("transcription_api_keys") || {};
  const usesLocalProvider = transcriptionProviderId === "local";
  const isElevenLabsSelected =
    transcriptionProviderId === ELEVENLABS_TRANSCRIPTION_PROVIDER_ID;
  const hasElevenLabsApiKey = hasTranscriptionProviderApiKey(
    ELEVENLABS_TRANSCRIPTION_PROVIDER_ID,
    transcriptionApiKeys,
  );
  const elevenLabsProfile = getElevenLabsModelProfile();

  // click outside handler for language dropdown
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        languageDropdownRef.current &&
        !languageDropdownRef.current.contains(event.target as Node)
      ) {
        setLanguageDropdownOpen(false);
        setLanguageSearch("");
      }
    };
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, []);

  // focus search input when dropdown opens
  useEffect(() => {
    if (languageDropdownOpen && languageSearchInputRef.current) {
      languageSearchInputRef.current.focus();
    }
  }, [languageDropdownOpen]);

  // filtered languages for dropdown (exclude "auto")
  const filteredLanguages = useMemo(() => {
    return LANGUAGES.filter(
      (lang) =>
        lang.value !== "auto" &&
        lang.label.toLowerCase().includes(languageSearch.toLowerCase()),
    );
  }, [languageSearch]);

  // Get selected language label
  const selectedLanguageLabel = useMemo(() => {
    if (languageFilter === "all") {
      return t("settings.models.filters.allLanguages");
    }
    return LANGUAGES.find((lang) => lang.value === languageFilter)?.label || "";
  }, [languageFilter, t]);

  const getModelStatus = (modelId: string): ModelCardStatus => {
    if (modelId in extractingModels) {
      return "extracting";
    }
    if (modelId in verifyingModels) {
      return "verifying";
    }
    if (modelId in downloadingModels) {
      return "downloading";
    }
    if (switchingModelId === modelId) {
      return "switching";
    }
    if (usesLocalProvider && modelId === currentModel) {
      return "active";
    }
    const model = models.find((m: ModelInfo) => m.id === modelId);
    if (model?.is_downloaded) {
      return "available";
    }
    return "downloadable";
  };

  const getDownloadProgress = (modelId: string): number | undefined => {
    const progress = downloadProgress[modelId];
    return progress?.percentage;
  };

  const getDownloadSpeed = (modelId: string): number | undefined => {
    const stats = downloadStats[modelId];
    return stats?.speed;
  };

  const handleModelSelect = async (modelId: string) => {
    setSwitchingModelId(modelId);
    try {
      await selectModel(modelId);
    } finally {
      setSwitchingModelId(null);
    }
  };

  const handleModelDownload = async (modelId: string) => {
    await downloadModel(modelId);
  };

  const handleModelDelete = async (modelId: string) => {
    const model = models.find((m: ModelInfo) => m.id === modelId);
    const modelName = model?.name || modelId;
    const isActive = modelId === currentModel;

    const confirmed = await ask(
      isActive
        ? t("settings.models.deleteActiveConfirm", { modelName })
        : t("settings.models.deleteConfirm", { modelName }),
      {
        title: t("settings.models.deleteTitle"),
        kind: "warning",
      },
    );

    if (confirmed) {
      try {
        await deleteModel(modelId);
      } catch (err) {
        console.error(`Failed to delete model ${modelId}:`, err);
      }
    }
  };

  const handleModelCancel = async (modelId: string) => {
    try {
      await cancelDownload(modelId);
    } catch (err) {
      console.error(`Failed to cancel download for ${modelId}:`, err);
    }
  };

  const getErrorMessage = (error: unknown) => {
    if (typeof error === "string") {
      return error;
    }

    if (error instanceof Error) {
      return error.message;
    }

    return t("settings.models.external.apiKey.saveFailed", {
      defaultValue: "Failed to save API key.",
    });
  };

  const handleElevenLabsSelect = async () => {
    setElevenLabsApiKeyError(null);
    await updateSetting(
      "transcription_provider_id",
      ELEVENLABS_TRANSCRIPTION_PROVIDER_ID,
    );
  };

  const handleElevenLabsApiKeySave = async (apiKey: string) => {
    const trimmedApiKey = apiKey.trim();

    if (!trimmedApiKey) {
      setElevenLabsApiKeyError(
        t("settings.models.external.apiKey.required", {
          defaultValue: "Enter your ElevenLabs API key.",
        }),
      );
      return;
    }

    setElevenLabsApiKeySaving(true);
    setElevenLabsApiKeyError(null);

    try {
      await invoke("verify_transcription_provider_api_key", {
        providerId: ELEVENLABS_TRANSCRIPTION_PROVIDER_ID,
        apiKey: trimmedApiKey,
      });

      const saveResult = await commands.changeTranscriptionApiKeySetting(
        ELEVENLABS_TRANSCRIPTION_PROVIDER_ID,
        trimmedApiKey,
      );
      if (saveResult.status !== "ok") {
        throw new Error(saveResult.error);
      }

      await refreshSettings();
      await updateSetting(
        "transcription_provider_id",
        ELEVENLABS_TRANSCRIPTION_PROVIDER_ID,
      );
    } catch (error) {
      setElevenLabsApiKeyError(getErrorMessage(error));
    } finally {
      setElevenLabsApiKeySaving(false);
    }
  };

  const handleElevenLabsApiKeyRemove = async () => {
    setElevenLabsApiKeySaving(true);
    setElevenLabsApiKeyError(null);

    try {
      if (isElevenLabsSelected) {
        await updateSetting("transcription_provider_id", "local");
      }

      const saveResult = await commands.changeTranscriptionApiKeySetting(
        ELEVENLABS_TRANSCRIPTION_PROVIDER_ID,
        "",
      );
      if (saveResult.status !== "ok") {
        throw new Error(saveResult.error);
      }

      await refreshSettings();
    } catch (error) {
      setElevenLabsApiKeyError(getErrorMessage(error));
    } finally {
      setElevenLabsApiKeySaving(false);
    }
  };

  // Filter models based on language filter
  const filteredModels = useMemo(() => {
    return models.filter((model: ModelInfo) => {
      if (languageFilter !== "all") {
        if (!modelSupportsLanguage(model, languageFilter)) return false;
      }
      return true;
    });
  }, [models, languageFilter]);

  // Split filtered models into downloaded (including custom) and available sections
  const { downloadedModels, availableModels } = useMemo(() => {
    const downloaded: ModelInfo[] = [];
    const available: ModelInfo[] = [];

    for (const model of filteredModels) {
      if (
        model.is_custom ||
        model.is_downloaded ||
        model.id in downloadingModels ||
        model.id in extractingModels
      ) {
        downloaded.push(model);
      } else {
        available.push(model);
      }
    }

    // Sort: active model first, then non-custom, then custom at the bottom
    downloaded.sort((a, b) => {
      if (usesLocalProvider && a.id === currentModel) return -1;
      if (usesLocalProvider && b.id === currentModel) return 1;
      if (a.is_custom !== b.is_custom) return a.is_custom ? 1 : -1;
      return 0;
    });

    return {
      downloadedModels: downloaded,
      availableModels: available,
    };
  }, [filteredModels, downloadingModels, extractingModels, currentModel]);

  if (loading) {
    return (
      <div className="max-w-3xl w-full mx-auto">
        <div className="flex items-center justify-center py-16">
          <div className="w-8 h-8 border-2 border-logo-primary border-t-transparent rounded-full animate-spin" />
        </div>
      </div>
    );
  }

  return (
    <div className="max-w-3xl w-full mx-auto space-y-4">
      <div className="mb-4">
        <h1 className="text-xl font-semibold mb-2">
          {t("settings.models.title")}
        </h1>
        <p className="text-sm text-text/60">
          {t("settings.models.description")}
        </p>
      </div>
      <div className="space-y-6">
        <div className="space-y-3">
          <div className="flex items-center justify-between">
            <h2 className="text-sm font-medium text-text/60">
              {t("settings.models.yourModels")}
            </h2>
            <div className="relative" ref={languageDropdownRef}>
              <button
                type="button"
                onClick={() => setLanguageDropdownOpen(!languageDropdownOpen)}
                className={`flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-lg transition-colors ${
                  languageFilter !== "all"
                    ? "bg-logo-primary/20 text-logo-primary"
                    : "bg-mid-gray/10 text-text/60 hover:bg-mid-gray/20"
                }`}
              >
                <Globe className="w-3.5 h-3.5" />
                <span className="max-w-[120px] truncate">
                  {selectedLanguageLabel}
                </span>
                <ChevronDown
                  className={`w-3.5 h-3.5 transition-transform ${
                    languageDropdownOpen ? "rotate-180" : ""
                  }`}
                />
              </button>

              {languageDropdownOpen && (
                <div className="absolute top-full right-0 mt-1 w-56 bg-background border border-mid-gray/80 rounded-lg shadow-lg z-50 overflow-hidden">
                  <div className="p-2 border-b border-mid-gray/40">
                    <input
                      ref={languageSearchInputRef}
                      type="text"
                      value={languageSearch}
                      onChange={(e) => setLanguageSearch(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter" && filteredLanguages.length > 0) {
                          setLanguageFilter(filteredLanguages[0].value);
                          setLanguageDropdownOpen(false);
                          setLanguageSearch("");
                        } else if (e.key === "Escape") {
                          setLanguageDropdownOpen(false);
                          setLanguageSearch("");
                        }
                      }}
                      placeholder={t(
                        "settings.general.language.searchPlaceholder",
                      )}
                      className="w-full px-2 py-1 text-sm bg-mid-gray/10 border border-mid-gray/40 rounded-md focus:outline-none focus:ring-1 focus:ring-logo-primary"
                    />
                  </div>
                  <div className="max-h-48 overflow-y-auto">
                    <button
                      type="button"
                      onClick={() => {
                        setLanguageFilter("all");
                        setLanguageDropdownOpen(false);
                        setLanguageSearch("");
                      }}
                      className={`w-full px-3 py-1.5 text-sm text-left transition-colors ${
                        languageFilter === "all"
                          ? "bg-logo-primary/20 text-logo-primary font-semibold"
                          : "hover:bg-mid-gray/10"
                      }`}
                    >
                      {t("settings.models.filters.allLanguages")}
                    </button>
                    {filteredLanguages.map((lang) => (
                      <button
                        key={lang.value}
                        type="button"
                        onClick={() => {
                          setLanguageFilter(lang.value);
                          setLanguageDropdownOpen(false);
                          setLanguageSearch("");
                        }}
                        className={`w-full px-3 py-1.5 text-sm text-left transition-colors ${
                          languageFilter === lang.value
                            ? "bg-logo-primary/20 text-logo-primary font-semibold"
                            : "hover:bg-mid-gray/10"
                        }`}
                      >
                        {lang.label}
                      </button>
                    ))}
                    {filteredLanguages.length === 0 && (
                      <div className="px-3 py-2 text-sm text-text/50 text-center">
                        {t("settings.general.language.noResults")}
                      </div>
                    )}
                  </div>
                </div>
              )}
            </div>
          </div>
          {hasElevenLabsApiKey && (
            <ExternalTranscriptionModelCard
              profile={elevenLabsProfile}
              isActive={isElevenLabsSelected}
              isInstalled={true}
              savedApiKey={
                transcriptionApiKeys[ELEVENLABS_TRANSCRIPTION_PROVIDER_ID] || ""
              }
              apiKeySaving={elevenLabsApiKeySaving}
              apiKeyError={elevenLabsApiKeyError}
              isSwitching={
                switchingModelId === ELEVENLABS_TRANSCRIPTION_PROVIDER_ID
              }
              onSelect={() => {
                setSwitchingModelId(ELEVENLABS_TRANSCRIPTION_PROVIDER_ID);
                void handleElevenLabsSelect().finally(() => {
                  setSwitchingModelId(null);
                });
              }}
              onSaveApiKey={handleElevenLabsApiKeySave}
              onRemoveApiKey={handleElevenLabsApiKeyRemove}
            />
          )}
          {downloadedModels.map((model: ModelInfo) => (
            <ModelCard
              key={model.id}
              model={model}
              status={getModelStatus(model.id)}
              onSelect={handleModelSelect}
              onDownload={handleModelDownload}
              onDelete={handleModelDelete}
              onCancel={handleModelCancel}
              downloadProgress={getDownloadProgress(model.id)}
              downloadSpeed={getDownloadSpeed(model.id)}
              showRecommended={false}
            />
          ))}
        </div>

        {availableModels.length > 0 && (
          <div className="space-y-3">
            <h2 className="text-sm font-medium text-text/60">
              {t("settings.models.availableModels")}
            </h2>
            {availableModels.map((model: ModelInfo) => (
              <ModelCard
                key={model.id}
                model={model}
                status={getModelStatus(model.id)}
                onSelect={handleModelSelect}
                onDownload={handleModelDownload}
                onDelete={handleModelDelete}
                onCancel={handleModelCancel}
                downloadProgress={getDownloadProgress(model.id)}
                downloadSpeed={getDownloadSpeed(model.id)}
                showRecommended={false}
              />
            ))}
          </div>
        )}

        {!hasElevenLabsApiKey && (
          <div className="space-y-3">
            <h2 className="text-sm font-medium text-text/60">
              {t("settings.models.cloudModels", {
                defaultValue: "Cloud Models",
              })}
            </h2>
            <ExternalTranscriptionModelCard
              profile={elevenLabsProfile}
              isActive={false}
              isInstalled={false}
              savedApiKey=""
              apiKeySaving={elevenLabsApiKeySaving}
              apiKeyError={elevenLabsApiKeyError}
              onSelect={handleElevenLabsSelect}
              onSaveApiKey={handleElevenLabsApiKeySave}
              onRemoveApiKey={handleElevenLabsApiKeyRemove}
            />
          </div>
        )}

        {languageFilter !== "all" && filteredModels.length === 0 && (
          <div className="text-center py-2 text-text/50">
            {t("settings.models.noModelsMatch")}
          </div>
        )}
      </div>
    </div>
  );
};
