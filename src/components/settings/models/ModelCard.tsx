import React from "react";
import { useTranslation } from "react-i18next";
import {
  Radio as LiveIcon,
  Check,
  Download,
  Globe,
  HardDrive,
  Languages,
  Loader2,
  Trash2,
} from "lucide-react";
import type { ModelInfo } from "@/bindings";
import { formatModelSize } from "@/lib/utils/format";
import {
  getTranslatedModelDescription,
  getTranslatedModelName,
} from "@/lib/utils/modelTranslation";
import { languageName, languagesOf } from "@/lib/constants/languages";
import Badge from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { useSettingsStore } from "@/stores/settingsStore";

// Get display text for model's language support
const getLanguageDisplayText = (
  supportedLanguages: string[],
  t: (key: string, options?: Record<string, unknown>) => string,
  uiLanguage: string,
): string => {
  const capabilityLanguages = languagesOf(supportedLanguages);
  if (capabilityLanguages.length === 1) {
    const langName = languageName(capabilityLanguages[0].code, uiLanguage);
    return t("modelSelector.capabilities.languageOnly", { language: langName });
  }
  return t("modelSelector.capabilities.languageCount", {
    total: capabilityLanguages.length,
  });
};

// Legacy models are the blob (Url-sourced) .bin downloads, superseded by
// the catalog GGUFs. They stay runnable when already on disk, but we no longer
// advertise the download.
export const isLegacyModel = (model: ModelInfo): boolean =>
  typeof model.source === "object" && "Url" in model.source;

// Extract a GGUF quantization label from a filename, if present (e.g. "Q8_0").
const getQuantLabel = (filename: string): string | null => {
  const match = filename.match(
    /[._-](IQ\d+_\w+|Q\d+(?:_\w+)?|F16|BF16|F32)\.gguf$/i,
  );
  return match ? match[1].toUpperCase() : null;
};

export type ModelCardStatus =
  | "downloadable"
  | "downloading"
  | "verifying"
  | "switching"
  | "active"
  | "available";

interface ModelCardProps {
  model: ModelInfo;
  status?: ModelCardStatus;
  onSelect: (modelId: string) => void;
  onDownload?: (modelId: string) => void;
  onDelete?: (modelId: string) => void;
  onCancel?: (modelId: string) => void;
  downloadProgress?: number;
  downloadSpeed?: number; // MB/s
  showRecommended?: boolean;
}

const ModelCard: React.FC<ModelCardProps> = ({
  model,
  status = "downloadable",
  onSelect,
  onDownload,
  onDelete,
  onCancel,
  downloadProgress,
  downloadSpeed,
  showRecommended = true,
}) => {
  const { t, i18n } = useTranslation();
  const debugMode = useSettingsStore(
    (state) => state.settings?.debug_mode ?? false,
  );
  // The active model is already loaded — re-selecting it just reloads it for no
  // gain, so it is deliberately not clickable.
  const isClickable = status === "available" || status === "downloadable";

  // Get translated model name and description
  const displayName = getTranslatedModelName(model, t);
  const displayDescription = getTranslatedModelDescription(model, t);
  const showModelSize =
    status === "downloadable" || status === "available" || status === "active";
  const formattedModelSize = formatModelSize(Number(model.size_mb));
  const quantLabel = getQuantLabel(model.filename);
  const capabilityLanguages = languagesOf(model.supported_languages);

  const baseClasses =
    "flex flex-col rounded-xl px-4 py-3 gap-2 text-left transition-all duration-200";

  const getVariantClasses = () => {
    if (status === "active") {
      return "border-2 border-logo-primary/50 bg-logo-primary/10";
    }
    return "border-2 border-line";
  };

  const getInteractiveClasses = () => {
    if (!isClickable) return "";
    return "cursor-pointer hover:border-logo-primary/50 hover:bg-logo-primary/10 hover:shadow-lg hover:scale-[1.01] active:scale-[0.99] group";
  };

  const handleClick = () => {
    if (!isClickable) return;
    if (status === "downloadable" && onDownload) {
      onDownload(model.id);
    } else {
      onSelect(model.id);
    }
  };

  const handleDelete = (e: React.MouseEvent) => {
    e.stopPropagation();
    onDelete?.(model.id);
  };

  return (
    <div
      onClick={handleClick}
      onKeyDown={(e) => {
        if (e.key === "Enter" && isClickable) handleClick();
      }}
      role={isClickable ? "button" : undefined}
      tabIndex={isClickable ? 0 : undefined}
      className={[baseClasses, getVariantClasses(), getInteractiveClasses()]
        .filter(Boolean)
        .join(" ")}
    >
      {/* Top section: name/description + score bars */}
      <div className="flex justify-between items-center w-full">
        <div className="flex flex-col items-start flex-1 min-w-0">
          <div className="flex items-center gap-3 flex-wrap">
            <h3
              className={`text-base font-semibold text-text ${isClickable ? "group-hover:text-logo-primary" : ""} transition-colors`}
            >
              {displayName}
            </h3>
            {showRecommended && model.is_recommended && (
              <Badge variant="primary">{t("onboarding.recommended")}</Badge>
            )}
            {status === "active" && (
              <Badge variant="primary">
                <Check className="w-3 h-3 mr-1" />
                {t("modelSelector.active")}
              </Badge>
            )}
            {model.is_custom && (
              <Badge variant="secondary">{t("modelSelector.custom")}</Badge>
            )}
            {isLegacyModel(model) && (
              <Badge variant="secondary">{t("modelSelector.legacy")}</Badge>
            )}
            {status === "switching" && (
              <Badge variant="secondary">
                <Loader2 className="w-3 h-3 mr-1 animate-spin" />
                {t("modelSelector.switching")}
              </Badge>
            )}
          </div>
          <p className="text-ink-2 text-sm leading-relaxed">
            {displayDescription}
          </p>
        </div>
        {(model.accuracy_score > 0 || model.speed_score > 0) && (
          <div className="hidden sm:flex items-center ms-4">
            <div className="space-y-1">
              <div className="flex items-center gap-2">
                <p className="text-xs text-ink-2 w-24 text-end">
                  {t("onboarding.modelCard.accuracy")}
                </p>
                <div className="w-16 h-1.5 bg-line rounded-full overflow-hidden">
                  <div
                    className="h-full bg-logo-primary rounded-full"
                    style={{ width: `${model.accuracy_score * 100}%` }}
                  />
                </div>
              </div>
              <div className="flex items-center gap-2">
                <p className="text-xs text-ink-2 w-24 text-end">
                  {t("onboarding.modelCard.speed")}
                </p>
                <div className="w-16 h-1.5 bg-line rounded-full overflow-hidden">
                  <div
                    className="h-full bg-logo-primary rounded-full"
                    style={{ width: `${model.speed_score * 100}%` }}
                  />
                </div>
              </div>
            </div>
          </div>
        )}
      </div>

      <hr className="w-full border-line" />

      {/* Bottom row: tags + action buttons (full width) */}
      <div className="flex items-center gap-3 w-full -mb-0.5 mt-0.5 h-5">
        {capabilityLanguages.length > 0 && (
          <div
            className="flex items-center gap-1 text-xs text-ink-3"
            title={
              capabilityLanguages.length === 1
                ? t("modelSelector.capabilities.singleLanguage")
                : t("modelSelector.capabilities.languageSelection")
            }
          >
            <Globe className="w-3.5 h-3.5" />
            <span>
              {getLanguageDisplayText(
                model.supported_languages,
                t,
                i18n.language,
              )}
            </span>
          </div>
        )}
        {model.supports_translation && (
          <div
            className="flex items-center gap-1 text-xs text-ink-3"
            title={t("modelSelector.capabilities.translation")}
          >
            <Languages className="w-3.5 h-3.5" />
            <span>{t("modelSelector.capabilities.translate")}</span>
          </div>
        )}
        {model.supports_streaming && (
          <div
            className="flex items-center gap-1 text-xs text-ink-3"
            title={t("modelSelector.capabilities.streaming")}
          >
            <LiveIcon className="w-3.5 h-3.5" />
            <span>{t("modelSelector.streaming")}</span>
          </div>
        )}
        {showModelSize && (
          <span className="flex items-center gap-1.5 ms-auto text-xs text-ink-3">
            {status === "downloadable" ? (
              <Download className="w-3.5 h-3.5" />
            ) : (
              <HardDrive className="w-3.5 h-3.5" />
            )}
            <span>{formattedModelSize}</span>
            {debugMode && quantLabel && (
              <span className="text-ink-3">{quantLabel}</span>
            )}
          </span>
        )}
        {onDelete && (status === "available" || status === "active") && (
          <Button
            variant="ghost"
            size="sm"
            onClick={handleDelete}
            title={t("modelSelector.deleteModel", { modelName: displayName })}
            className="flex items-center gap-1.5 text-logo-primary hover:text-logo-primary hover:bg-logo-primary/10"
          >
            <Trash2 className="w-3.5 h-3.5" />
            <span>{t("common.delete")}</span>
          </Button>
        )}
      </div>

      {/* Download progress */}
      {status === "downloading" && downloadProgress !== undefined && (
        <div className="w-full mt-3">
          <div className="w-full h-1.5 bg-line rounded-full overflow-hidden">
            <div
              className="h-full bg-logo-primary rounded-full transition-all duration-300"
              style={{ width: `${downloadProgress}%` }}
            />
          </div>
          <div className="flex items-center justify-between text-xs mt-1">
            <span className="text-ink-3">
              {t("modelSelector.downloading", {
                percentage: Math.round(downloadProgress),
              })}
            </span>
            <div className="flex items-center gap-2">
              {downloadSpeed !== undefined && downloadSpeed > 0 && (
                <span className="tabular-nums text-ink-3">
                  {t("modelSelector.downloadSpeed", {
                    speed: downloadSpeed.toFixed(1),
                  })}
                </span>
              )}
              {onCancel && (
                <Button
                  variant="danger-ghost"
                  size="sm"
                  onClick={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                    onCancel(model.id);
                  }}
                  aria-label={t("modelSelector.cancelDownload")}
                >
                  {t("modelSelector.cancel")}
                </Button>
              )}
            </div>
          </div>
        </div>
      )}
      {status === "verifying" && (
        <div className="w-full mt-3">
          <div className="w-full h-1.5 bg-line rounded-full overflow-hidden">
            <div className="h-full bg-logo-primary rounded-full animate-pulse w-full" />
          </div>
          <p className="text-xs text-ink-3 mt-1">
            {t("modelSelector.verifyingGeneric")}
          </p>
        </div>
      )}
    </div>
  );
};

export default ModelCard;
