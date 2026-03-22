import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ChevronDown,
  Download,
  ExternalLink,
  Heart,
  Loader2,
  Search,
  Globe,
} from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { HFLogo } from "./HFLogo";
import { useModelStore } from "@/stores/modelStore";
import type { HFModelResult } from "@/bindings";
import { Button } from "@/components/ui/Button";
import { useRef, useEffect } from "react";
import { LANGUAGES } from "@/lib/constants/languages";
import { LanguageList } from "@/components/ui";

export const HFModelBrowser: React.FC = () => {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<HFModelResult[]>([]);
  const [searching, setSearching] = useState(false);
  const {
    searchHFModels,
    downloadHFModel,
    downloadingModels,
    downloadProgress,
  } = useModelStore();

  const sortOptions = [
    {
      value: "trendingScore",
      label: t("settings.models.huggingface.sort.trending", "Trending"),
    },
    {
      value: "likes",
      label: t("settings.models.huggingface.sort.mostLiked", "Most Liked"),
    },
    {
      value: "downloads",
      label: t(
        "settings.models.huggingface.sort.mostDownloaded",
        "Most Downloaded",
      ),
    },
    {
      value: "createdAt",
      label: t(
        "settings.models.huggingface.sort.recentlyCreated",
        "Recently Created",
      ),
    },
    {
      value: "lastModified",
      label: t(
        "settings.models.huggingface.sort.recentlyUpdated",
        "Recently Updated",
      ),
    },
  ];

  const [sortBy, setSortBy] = useState("downloads");
  const [dropdownOpen, setDropdownOpen] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);

  // click outside handler
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        dropdownRef.current &&
        !dropdownRef.current.contains(event.target as Node)
      ) {
        setDropdownOpen(false);
      }
    };
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, []);

  const handleSearch = async (e?: React.FormEvent, customSort?: string) => {
    if (e) e.preventDefault();
    if (!query.trim()) return;

    const currentSort = customSort || sortBy;
    setSearching(true);
    try {
      const models = await searchHFModels(query, currentSort);
      setResults(models);
    } finally {
      setSearching(false);
    }
  };

  const handleSortChange = (value: string) => {
    setSortBy(value);
    setDropdownOpen(false);
    if (query.trim()) {
      handleSearch(undefined, value);
    }
  };

  const handleDownload = async (modelId: string) => {
    await downloadHFModel(modelId);
  };

  return (
    <div className="space-y-6">
      <div className="flex flex-col gap-4">
        <form onSubmit={handleSearch} className="flex gap-2 items-center">
          <div className="relative flex-1 group">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-text/40 group-focus-within:text-logo-primary transition-colors" />
            <input
              placeholder={t(
                "settings.models.huggingface.searchPlaceholder",
                "e.g. distil-medium, whisper-v3...",
              )}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              className="w-full pl-10 pr-4 py-2 text-sm bg-mid-gray/5 border border-mid-gray/20 rounded-xl focus:outline-none focus:border-logo-primary/50 focus:ring-4 focus:ring-logo-primary/5 transition-all placeholder:text-text/20"
            />
          </div>

          <div className="relative shrink-0" ref={dropdownRef}>
            <button
              type="button"
              onClick={() => setDropdownOpen(!dropdownOpen)}
              className="flex items-center gap-1.5 h-8 px-2.5 text-[9px] font-bold text-text/50 uppercase tracking-widest bg-mid-gray/10 hover:bg-mid-gray/20 border border-mid-gray/20 rounded-lg transition-colors whitespace-nowrap"
            >
              <span>{sortOptions.find((o) => o.value === sortBy)?.label}</span>
              <ChevronDown
                className={`w-2.5 h-2.5 transition-transform ${dropdownOpen ? "rotate-180" : ""}`}
              />
            </button>

            {dropdownOpen && (
              <div className="absolute top-full right-0 mt-1 w-48 bg-background border border-mid-gray/80 rounded-xl shadow-xl z-50 overflow-hidden">
                <div className="py-1">
                  {sortOptions.map((option) => (
                    <button
                      key={option.value}
                      type="button"
                      onClick={() => handleSortChange(option.value)}
                      className={`w-full px-4 py-2 text-xs text-left transition-colors ${
                        sortBy === option.value
                          ? "bg-logo-primary/20 text-logo-primary font-semibold"
                          : "text-text/80 hover:bg-mid-gray/10"
                      }`}
                    >
                      {option.label}
                    </button>
                  ))}
                </div>
              </div>
            )}
          </div>

          <Button
            type="submit"
            variant="primary"
            size="sm"
            disabled={searching || !query.trim()}
            className="h-8 px-2 rounded-lg shadow-sm hover:shadow transition-all active:scale-[0.98]"
          >
            {searching ? (
              <Loader2 className="w-3 h-3 animate-spin" />
            ) : (
              <div className="flex items-center gap-1">
                <Search className="w-3 h-3" />
                <span className="text-[10px] font-bold uppercase tracking-tight">
                  {t("common.search", "Search")}
                </span>
              </div>
            )}
          </Button>
        </form>

        <div className="flex items-center gap-2">
          <div className="w-1 h-3 bg-logo-primary/50 rounded-full" />
          <span className="text-[10px] font-bold text-text/30 uppercase tracking-[0.2em]">
            {results.length > 0
              ? t("settings.models.huggingface.resultsCount", {
                  count: results.length,
                })
              : t("settings.models.huggingface.browser", "Browser")}
          </span>
        </div>
      </div>

      <div className="grid gap-3">
        {results.length > 0
          ? results.map((model) => {
              const isDownloading = model.id in downloadingModels;
              const progress = downloadProgress[model.id]?.percentage || 0;

              const extractedLangs = model.tags
                .map((t) => t.replace("language:", ""))
                .filter(
                  (t) =>
                    (t.length === 2 && /^[a-z]+$/.test(t)) ||
                    t === "zh-Hans" ||
                    t === "zh-Hant",
                );

              return (
                <div
                  key={model.id}
                  className="flex flex-col rounded-xl px-4 py-3 gap-2 border-2 border-mid-gray/20 hover:border-logo-primary/50 hover:bg-logo-primary/5 hover:shadow-lg hover:scale-[1.01] active:scale-[0.99] transition-all duration-200 cursor-pointer group"
                  onClick={() => openUrl(`https://huggingface.co/${model.id}`)}
                >
                  <div className="flex justify-between items-center w-full">
                    <div className="flex flex-col items-start flex-1 min-w-0 pr-2">
                      <div className="flex items-center gap-3 flex-wrap">
                        <h3 className="text-base font-semibold text-text group-hover:text-logo-primary transition-colors truncate max-w-full">
                          {model.id}
                        </h3>
                        <div className="text-text/30 group-hover:text-logo-primary transition-colors">
                          <ExternalLink className="w-3.5 h-3.5" />
                        </div>
                      </div>
                      <p className="text-text/60 text-sm leading-relaxed line-clamp-1">
                        {t(
                          "settings.models.huggingface.repo",
                          "Community Repository",
                        )}
                      </p>
                    </div>

                    {!isDownloading && (
                      <Button
                        size="sm"
                        variant="primary"
                        onClick={(e) => {
                          e.stopPropagation();
                          handleDownload(model.id);
                        }}
                        className="ml-4 h-7 px-3 rounded-md flex items-center gap-1.5 shadow-sm hover:shadow-logo-primary/20 transition-all active:scale-95"
                      >
                        <Download className="w-3 h-3 shrink-0" />
                        <span className="text-[11px] font-bold uppercase tracking-wider">
                          {t("common.download", "Download")}
                        </span>
                      </Button>
                    )}
                  </div>

                  <hr className="w-full border-mid-gray/20" />

                  <div className="flex items-center gap-4 text-xs text-text/50 h-5">
                    <div className="flex items-center gap-1.5">
                      <Download className="w-3.5 h-3.5" />
                      <span>{model.downloads.toLocaleString()}</span>
                    </div>
                    <div className="flex items-center gap-1.5">
                      <Heart className="w-3.5 h-3.5 text-red-500/60" />
                      <span>{model.likes}</span>
                    </div>
                    {extractedLangs.length > 0 && (
                      <div className="flex items-center gap-1.5 ml-2 border-l border-mid-gray/20 pl-4">
                        <LanguageList languages={extractedLangs} />
                      </div>
                    )}
                    {model.last_modified && (
                      <span className="ml-auto">
                        {new Date(model.last_modified).toLocaleDateString()}
                      </span>
                    )}
                  </div>

                  {isDownloading && (
                    <div className="w-full mt-2">
                      <div className="w-full h-1.5 bg-mid-gray/20 rounded-full overflow-hidden">
                        <div
                          className="h-full bg-logo-primary rounded-full transition-all duration-300"
                          style={{ width: `${progress}%` }}
                        />
                      </div>
                      <p className="text-xs text-text/50 mt-1 text-center">
                        {t("modelSelector.downloading", {
                          percentage: Math.round(progress),
                        })}
                      </p>
                    </div>
                  )}
                </div>
              );
            })
          : !searching &&
            query && (
              <div className="text-center py-12 bg-mid-gray/5 border-2 border-dashed border-mid-gray/10 rounded-xl">
                <p className="text-text/40 text-sm italic">
                  {t(
                    "settings.models.huggingface.noResults",
                    "No community models found for this search.",
                  )}
                </p>
              </div>
            )}
      </div>
    </div>
  );
};
