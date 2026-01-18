import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown } from "lucide-react";
import type { ModelInfo } from "@/bindings";
import { formatModelSize } from "../../lib/utils/format";
import {
  getTranslatedModelName,
  getTranslatedModelDescription,
} from "../../lib/utils/modelTranslation";
import { ProgressBar } from "../shared";

interface DownloadProgress {
  model_id: string;
  downloaded: number;
  total: number;
  percentage: number;
}

interface ModelDropdownProps {
  models: ModelInfo[];
  currentModelId: string;
  downloadProgress: Record<string, DownloadProgress>;
  onModelSelect: (modelId: string) => void;
  onModelDownload: (modelId: string) => void;
  onModelDelete: (modelId: string) => Promise<void>;
  onError?: (error: string) => void;
}

const ModelDropdown: React.FC<ModelDropdownProps> = ({
  models,
  currentModelId,
  downloadProgress,
  onModelSelect,
  onModelDownload,
  onModelDelete,
  onError,
}) => {
  const { t } = useTranslation();

  // Split models into three groups: downloaded (with URL), custom (no URL), downloadable
  const downloadedModels = models.filter(
    (m) => m.is_downloaded && m.url !== null,
  );
  const customModels = models.filter((m) => m.url === null);
  const downloadableModels = models.filter(
    (m) => !m.is_downloaded && m.url !== null,
  );

  const hasDownloadedModels =
    downloadedModels.length > 0 || customModels.length > 0;
  const isFirstRun = !hasDownloadedModels && models.length > 0;

  // Collapse downloadable section by default when there are downloaded models
  const [downloadableExpanded, setDownloadableExpanded] = useState(isFirstRun);

  const handleDeleteClick = async (e: React.MouseEvent, modelId: string) => {
    e.preventDefault();
    e.stopPropagation();

    try {
      await onModelDelete(modelId);
    } catch (err) {
      const errorMsg = `Failed to delete model: ${err}`;
      onError?.(errorMsg);
    }
  };

  const handleModelClick = (modelId: string) => {
    if (modelId in downloadProgress) {
      return; // Don't allow interaction while downloading
    }
    onModelSelect(modelId);
  };

  const handleDownloadClick = (modelId: string) => {
    if (modelId in downloadProgress) {
      return; // Don't allow interaction while downloading
    }
    onModelDownload(modelId);
  };

  // Reusable model item renderer for downloaded/custom models
  const renderModelItem = (model: ModelInfo, isCustom = false) => (
    <div
      key={model.id}
      onClick={() => handleModelClick(model.id)}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          handleModelClick(model.id);
        }
      }}
      tabIndex={0}
      role="button"
      className={`w-full px-3 py-2 text-left hover:bg-mid-gray/10 transition-colors cursor-pointer focus:outline-none ${
        currentModelId === model.id
          ? "bg-logo-primary/10 text-logo-primary"
          : ""
      }`}
    >
      <div className="flex items-center justify-between">
        <div>
          <div className="text-sm flex items-center gap-2">
            <span>{getTranslatedModelName(model, t)}</span>
            {isCustom && (
              <span className="text-xs bg-mid-gray/20 text-text/60 px-1.5 py-0.5 rounded">
                {t("modelSelector.custom")}
              </span>
            )}
          </div>
          <div className="text-xs text-text/40 italic pr-4">
            {getTranslatedModelDescription(model, t)}
          </div>
        </div>
        <div className="flex items-center gap-2">
          {currentModelId === model.id && (
            <div className="text-xs text-logo-primary">
              {t("modelSelector.active")}
            </div>
          )}
          {currentModelId !== model.id && (
            <button
              onClick={(e) => handleDeleteClick(e, model.id)}
              className="text-red-400 hover:text-red-300 p-1 hover:bg-red-500/10 rounded transition-colors"
              title={t("modelSelector.deleteModel", {
                modelName: getTranslatedModelName(model, t),
              })}
            >
              <svg className="w-3 h-3" fill="currentColor" viewBox="0 0 20 20">
                <path
                  fillRule="evenodd"
                  d="M9 2a1 1 0 00-.894.553L7.382 4H4a1 1 0 000 2v10a2 2 0 002 2h8a2 2 0 002-2V6a1 1 0 100-2h-3.382l-.724-1.447A1 1 0 0011 2H9zM7 8a1 1 0 012 0v6a1 1 0 11-2 0V8zm5-1a1 1 0 00-1 1v6a1 1 0 102 0V8a1 1 0 00-1-1z"
                  clipRule="evenodd"
                />
              </svg>
            </button>
          )}
        </div>
      </div>
    </div>
  );

  return (
    <div className="absolute bottom-full left-0 mb-2 w-72 bg-background border border-mid-gray/20 rounded-lg shadow-lg py-2 z-50 max-h-96 overflow-y-auto">
      {/* First Run Welcome */}
      {isFirstRun && (
        <div className="px-3 py-2 bg-logo-primary/10 border-b border-logo-primary/20">
          <div className="text-xs font-medium text-logo-primary mb-1">
            {t("modelSelector.welcome")}
          </div>
          <div className="text-xs text-text/70">
            {t("modelSelector.downloadPrompt")}
          </div>
        </div>
      )}

      {/* Custom Models */}
      {customModels.length > 0 && (
        <div>
          <div className="px-3 py-1.5 text-xs font-medium text-text/80 border-b border-mid-gray/10">
            {t("modelSelector.customModels")} ({customModels.length})
          </div>
          {customModels.map((model) => renderModelItem(model, true))}
        </div>
      )}

      {/* Downloaded Models */}
      {downloadedModels.length > 0 && (
        <div>
          {customModels.length > 0 && (
            <div className="border-t border-mid-gray/10 my-1" />
          )}
          <div className="px-3 py-1.5 text-xs font-medium text-text/80 border-b border-mid-gray/10">
            {t("modelSelector.downloadedModels")} ({downloadedModels.length})
          </div>
          {downloadedModels.map((model) => renderModelItem(model, false))}
        </div>
      )}

      {/* Downloadable Models - Collapsible */}
      {downloadableModels.length > 0 && (
        <div>
          {(hasDownloadedModels || isFirstRun) && (
            <div className="border-t border-mid-gray/10 my-1" />
          )}
          <button
            type="button"
            onClick={() => setDownloadableExpanded(!downloadableExpanded)}
            className="w-full px-3 py-1.5 text-xs font-medium text-text/80 hover:bg-mid-gray/5 flex items-center justify-between transition-colors"
          >
            <span>
              {isFirstRun
                ? t("modelSelector.chooseModel")
                : t("modelSelector.downloadModels")}{" "}
              ({downloadableModels.length})
            </span>
            <ChevronDown
              className={`w-4 h-4 text-text/50 transition-transform duration-200 ${
                downloadableExpanded ? "rotate-180" : ""
              }`}
            />
          </button>
          {downloadableExpanded &&
            downloadableModels.map((model) => {
              const isDownloading = model.id in downloadProgress;
              const progress = downloadProgress[model.id];

              return (
                <div
                  key={model.id}
                  onClick={() => handleDownloadClick(model.id)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" || e.key === " ") {
                      e.preventDefault();
                      handleDownloadClick(model.id);
                    }
                  }}
                  tabIndex={0}
                  role="button"
                  aria-disabled={isDownloading}
                  className={`w-full px-3 py-2 text-left hover:bg-mid-gray/10 transition-colors cursor-pointer focus:outline-none ${
                    isDownloading
                      ? "opacity-50 cursor-not-allowed hover:bg-transparent"
                      : ""
                  }`}
                >
                  <div className="flex items-center justify-between">
                    <div className="min-w-0 flex-1">
                      <div className="text-sm">
                        {getTranslatedModelName(model, t)}
                        {model.id === "parakeet-tdt-0.6b-v3" && isFirstRun && (
                          <span className="ml-2 text-xs bg-logo-primary/20 text-logo-primary px-1.5 py-0.5 rounded">
                            {t("onboarding.recommended")}
                          </span>
                        )}
                      </div>
                      <div className="text-xs text-text/40 italic truncate pr-4">
                        {getTranslatedModelDescription(model, t)}
                      </div>
                      <div className="mt-1 text-xs text-text/50 tabular-nums">
                        {t("modelSelector.downloadSize")} ·{" "}
                        {formatModelSize(Number(model.size_mb))}
                      </div>
                    </div>
                    <div className="text-xs text-logo-primary tabular-nums shrink-0">
                      {isDownloading && progress
                        ? `${Math.max(0, Math.min(100, Math.round(progress.percentage)))}%`
                        : t("modelSelector.download")}
                    </div>
                  </div>

                  {isDownloading && progress && (
                    <div className="mt-2">
                      <ProgressBar
                        progress={[
                          {
                            id: model.id,
                            percentage: progress.percentage,
                            label: model.name,
                          },
                        ]}
                        size="small"
                      />
                    </div>
                  )}
                </div>
              );
            })}
        </div>
      )}

      {/* No Models Available */}
      {!hasDownloadedModels && downloadableModels.length === 0 && (
        <div className="px-3 py-2 text-sm text-text/60">
          {t("modelSelector.noModelsAvailable")}
        </div>
      )}
    </div>
  );
};

export default ModelDropdown;
