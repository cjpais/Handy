import React, { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ask } from "@tauri-apps/plugin-dialog";
import { ChevronDown, Globe, Languages } from "lucide-react";
import type { ModelCardStatus } from "@/components/onboarding";
import { ModelCard } from "@/components/onboarding";
import { useModels } from "@/hooks/useModels.ts";
import { LANGUAGES } from "@/lib/constants/languages.ts";
import type { ModelInfo } from "@/bindings";

type ModelFilter = "all" | "multiLanguage" | "translation";

// check if model supports a language based on its capabilities
const modelSupportsLanguage = (model: ModelInfo, langCode: string): boolean => {
  // models with language selection support all languages in the LANGUAGES list, like Whisper
  if (model.supports_language_selection) {
    return true;
  }
  // models without language selection only support English, like Parakeet
  return langCode === "en";
};

export const ModelsSettings: React.FC = () => {
  const { t } = useTranslation();
  const [activeFilter, setActiveFilter] = useState<ModelFilter>("all");
  const [switchingModelId, setSwitchingModelId] = useState<string | null>(null);
  const [languageFilter, setLanguageFilter] = useState("all");
  const [languageDropdownOpen, setLanguageDropdownOpen] = useState(false);
  const [languageSearch, setLanguageSearch] = useState("");
  const languageDropdownRef = useRef<HTMLDivElement>(null);
  const languageSearchInputRef = useRef<HTMLInputElement>(null);
  const {
    models,
    currentModel,
    downloadingModels,
    downloadProgress,
    downloadStats,
    extractingModels,
    loading,
    downloadModel,
    selectModel,
    deleteModel,
  } = useModels();

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
    if (extractingModels.has(modelId)) {
      return "extracting";
    }
    if (downloadingModels.has(modelId)) {
      return "downloading";
    }
    if (switchingModelId === modelId) {
      return "switching";
    }
    if (modelId === currentModel) {
      return "active";
    }
    const model = models.find((m) => m.id === modelId);
    if (model?.is_downloaded) {
      return "available";
    }
    return "downloadable";
  };

  const getDownloadProgress = (modelId: string): number | undefined => {
    const progress = downloadProgress.get(modelId);
    return progress?.percentage;
  };

  const getDownloadSpeed = (modelId: string): number | undefined => {
    const stats = downloadStats.get(modelId);
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
    const model = models.find((m) => m.id === modelId);
    const modelName = model?.name || modelId;

    const confirmed = await ask(
      t("settings.models.deleteConfirm", { modelName }),
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

  // Filter models based on active filter and language filter
  const filteredModels = useMemo(() => {
    return models.filter((model) => {
      // Capability filters
      switch (activeFilter) {
        case "multiLanguage":
          if (!model.supports_language_selection) return false;
          break;
        case "translation":
          if (!model.supports_translation) return false;
          break;
      }

      // Language filter
      if (languageFilter !== "all") {
        if (!modelSupportsLanguage(model, languageFilter)) return false;
      }

      return true;
    });
  }, [models, activeFilter, languageFilter]);

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
      <div className="mb-6">
        <h1 className="text-xl font-semibold mb-2">
          {t("settings.models.title")}
        </h1>
        <p className="text-sm text-text/60">
          {t("settings.models.description")}
        </p>
      </div>
      <div className="flex gap-2 mb-4">
        <button
          type="button"
          onClick={() => setActiveFilter("all")}
          className={`px-3 py-1.5 text-sm font-medium rounded-lg transition-colors ${
            activeFilter === "all"
              ? "bg-logo-primary/20 text-logo-primary"
              : "bg-mid-gray/10 text-text/60 hover:bg-mid-gray/20"
          }`}
        >
          {t("settings.models.filters.all")}
        </button>
        <button
          type="button"
          onClick={() => setActiveFilter("multiLanguage")}
          className={`flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-lg transition-colors ${
            activeFilter === "multiLanguage"
              ? "bg-logo-primary/20 text-logo-primary"
              : "bg-mid-gray/10 text-text/60 hover:bg-mid-gray/20"
          }`}
        >
          <Globe className="w-3.5 h-3.5" />
          {t("settings.models.filters.multiLanguage")}
        </button>
        <button
          type="button"
          onClick={() => setActiveFilter("translation")}
          className={`flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-lg transition-colors ${
            activeFilter === "translation"
              ? "bg-logo-primary/20 text-logo-primary"
              : "bg-mid-gray/10 text-text/60 hover:bg-mid-gray/20"
          }`}
        >
          <Languages className="w-3.5 h-3.5" />
          {t("settings.models.filters.translation")}
        </button>

        {/* Language filter dropdown */}
        <div className="relative ml-auto" ref={languageDropdownRef}>
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
                  placeholder={t("settings.general.language.searchPlaceholder")}
                  className="w-full px-2 py-1 text-sm bg-mid-gray/10 border border-mid-gray/40 rounded focus:outline-none focus:ring-1 focus:ring-logo-primary"
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
      {filteredModels.length > 0 ? (
        <div className="space-y-3">
          {filteredModels.map((model) => (
            <ModelCard
              key={model.id}
              model={model}
              status={getModelStatus(model.id)}
              variant={model.is_recommended ? "featured" : "default"}
              onSelect={handleModelSelect}
              onDownload={handleModelDownload}
              onDelete={handleModelDelete}
              downloadProgress={getDownloadProgress(model.id)}
              downloadSpeed={getDownloadSpeed(model.id)}
            />
          ))}
        </div>
      ) : (
        <div className="text-center py-8 text-text/50">
          {t("settings.models.noModelsMatch")}
        </div>
      )}
    </div>
  );
};
