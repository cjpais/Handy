import React, { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ask } from "@tauri-apps/plugin-dialog";
import {
  ArrowDown,
  ArrowUp,
  ArrowUpDown,
  Check,
  ChevronDown,
  Globe,
} from "lucide-react";
import type { ModelCardStatus } from "@/components/onboarding";
import { ModelCard } from "@/components/onboarding";
import { useModelStore } from "@/stores/modelStore";
import { LANGUAGES } from "@/lib/constants/languages.ts";
import type { ModelInfo } from "@/bindings";

// check if model supports a language based on its supported_languages list
const modelSupportsLanguage = (model: ModelInfo, langCode: string): boolean => {
  return model.supported_languages.includes(langCode);
};

type SortOption = "default" | "accuracy" | "speed" | "size";
type SortDirection = "desc" | "asc";

const applySortComparator = (
  a: ModelInfo,
  b: ModelInfo,
  sort: SortOption,
  direction: SortDirection,
): number => {
  if (sort === "size") {
    const diff = b.size_mb - a.size_mb;
    return direction === "asc" ? -diff : diff;
  }
  const key = sort === "accuracy" ? "accuracy_score" : "speed_score";
  if (a[key] === 0 && b[key] === 0) return 0;
  if (a[key] === 0) return 1;
  if (b[key] === 0) return -1;
  const diff = b[key] - a[key];
  return direction === "asc" ? -diff : diff;
};

export const ModelsSettings: React.FC = () => {
  const { t } = useTranslation();
  const [switchingModelId, setSwitchingModelId] = useState<string | null>(null);
  const [languageFilter, setLanguageFilter] = useState("all");
  const [languageDropdownOpen, setLanguageDropdownOpen] = useState(false);
  const [languageSearch, setLanguageSearch] = useState("");
  const languageDropdownRef = useRef<HTMLDivElement>(null);
  const languageSearchInputRef = useRef<HTMLInputElement>(null);
  const [sortBy, setSortBy] = useState<SortOption>("default");
  const [sortDirection, setSortDirection] = useState<SortDirection>("desc");
  const [sortDropdownOpen, setSortDropdownOpen] = useState(false);
  const sortDropdownRef = useRef<HTMLDivElement>(null);
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

  // click outside handler for sort dropdown
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        sortDropdownRef.current &&
        !sortDropdownRef.current.contains(event.target as Node)
      ) {
        setSortDropdownOpen(false);
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
    if (modelId === currentModel) {
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

    // Sort: active model first, then by selected sort option
    downloaded.sort((a, b) => {
      if (a.id === currentModel) return -1;
      if (b.id === currentModel) return 1;
      if (sortBy !== "default") {
        return applySortComparator(a, b, sortBy, sortDirection);
      }
      if (a.is_custom !== b.is_custom) return a.is_custom ? 1 : -1;
      return 0;
    });

    if (sortBy !== "default") {
      available.sort((a, b) =>
        applySortComparator(a, b, sortBy, sortDirection),
      );
    }

    return {
      downloadedModels: downloaded,
      availableModels: available,
    };
  }, [
    filteredModels,
    downloadingModels,
    extractingModels,
    currentModel,
    sortBy,
    sortDirection,
  ]);

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
      {filteredModels.length > 0 ? (
        <div className="space-y-6">
          {/* Downloaded Models Section — header always visible so filter stays accessible */}
          <div className="space-y-3">
            <div className="flex items-center justify-between">
              <h2 className="text-sm font-medium text-text/60">
                {t("settings.models.yourModels")}
              </h2>
              <div className="flex items-center gap-2">
                {/* Sort dropdown */}
                <div className="relative" ref={sortDropdownRef}>
                  <button
                    type="button"
                    onClick={() => setSortDropdownOpen(!sortDropdownOpen)}
                    className={`flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-lg transition-colors ${
                      sortBy !== "default"
                        ? "bg-logo-primary/20 text-logo-primary"
                        : "bg-mid-gray/10 text-text/60 hover:bg-mid-gray/20"
                    }`}
                  >
                    {sortBy !== "default" ? (
                      sortDirection === "desc" ? (
                        <ArrowDown className="w-3.5 h-3.5" />
                      ) : (
                        <ArrowUp className="w-3.5 h-3.5" />
                      )
                    ) : (
                      <ArrowUpDown className="w-3.5 h-3.5" />
                    )}
                    <span>{t(`settings.models.sort.${sortBy}`)}</span>
                    <ChevronDown
                      className={`w-3.5 h-3.5 transition-transform ${
                        sortDropdownOpen ? "rotate-180" : ""
                      }`}
                    />
                  </button>

                  {sortDropdownOpen && (
                    <div className="absolute top-full right-0 mt-1 w-44 bg-background border border-mid-gray/80 rounded-lg shadow-lg z-50 overflow-hidden py-1">
                      {(
                        ["default", "accuracy", "speed", "size"] as SortOption[]
                      ).map((option) => (
                        <button
                          key={option}
                          type="button"
                          onClick={() => {
                            if (option === sortBy && option !== "default") {
                              setSortDirection((d) =>
                                d === "desc" ? "asc" : "desc",
                              );
                            } else {
                              setSortBy(option);
                              setSortDirection("desc");
                            }
                            setSortDropdownOpen(false);
                          }}
                          className={`w-full px-3 py-1.5 text-sm text-left transition-colors flex items-center justify-between ${
                            sortBy === option
                              ? "bg-logo-primary/10 text-logo-primary"
                              : "hover:bg-mid-gray/10"
                          }`}
                        >
                          <span>{t(`settings.models.sort.${option}`)}</span>
                          {sortBy === option && option !== "default" && (
                            <span className="flex items-center gap-1 text-logo-primary/70">
                              {sortDirection === "desc" ? (
                                <ArrowDown className="w-3 h-3" />
                              ) : (
                                <ArrowUp className="w-3 h-3" />
                              )}
                            </span>
                          )}
                          {sortBy === option && option === "default" && (
                            <Check className="w-3 h-3 text-logo-primary/70" />
                          )}
                        </button>
                      ))}
                    </div>
                  )}
                </div>

                {/* Language filter dropdown */}
                <div className="relative" ref={languageDropdownRef}>
                  <button
                    type="button"
                    onClick={() =>
                      setLanguageDropdownOpen(!languageDropdownOpen)
                    }
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
                            if (
                              e.key === "Enter" &&
                              filteredLanguages.length > 0
                            ) {
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
                      <div className="max-h-48 overflow-y-auto py-1">
                        <button
                          type="button"
                          onClick={() => {
                            setLanguageFilter("all");
                            setLanguageDropdownOpen(false);
                            setLanguageSearch("");
                          }}
                          className={`w-full px-3 py-1.5 text-sm text-left transition-colors flex items-center justify-between ${
                            languageFilter === "all"
                              ? "bg-logo-primary/10 text-logo-primary"
                              : "hover:bg-mid-gray/10"
                          }`}
                        >
                          <span>
                            {t("settings.models.filters.allLanguages")}
                          </span>
                          {languageFilter === "all" && (
                            <Check className="w-3 h-3 text-logo-primary/70" />
                          )}
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
                            className={`w-full px-3 py-1.5 text-sm text-left transition-colors flex items-center justify-between ${
                              languageFilter === lang.value
                                ? "bg-logo-primary/10 text-logo-primary"
                                : "hover:bg-mid-gray/10"
                            }`}
                          >
                            <span>{lang.label}</span>
                            {languageFilter === lang.value && (
                              <Check className="w-3 h-3 text-logo-primary/70" />
                            )}
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
            </div>
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

          {/* Available Models Section */}
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
        </div>
      ) : (
        <div className="text-center py-8 text-text/50">
          {t("settings.models.noModelsMatch")}
        </div>
      )}
    </div>
  );
};
