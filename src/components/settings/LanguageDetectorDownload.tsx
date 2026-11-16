import React from "react";
import { useTranslation } from "react-i18next";
import { X } from "lucide-react";
import { DownloadMark } from "../shared/DownloadMark";
import { ProgressRing } from "../shared/ProgressRing";
import { Tooltip } from "../ui/Tooltip";
import { LANGUAGE_DETECTOR_SIZE } from "../../hooks/useLanguageDetector";

interface LanguageDetectorDownloadProps {
  downloading: boolean;
  percent: number;
  /** Explains the size or the progress on hover. */
  hint?: string;
  /** Offers a stop button while downloading, where nothing else can stop it. */
  onCancel?: () => void;
}

/** The end of a row whose choice downloads the language detector: its size, then its progress. */
export const LanguageDetectorDownload: React.FC<
  LanguageDetectorDownloadProps
> = ({ downloading, percent, hint, onCancel }) => {
  const { t } = useTranslation();
  const state = downloading ? (
    <span
      role="status"
      aria-label={t("modelSelector.downloading", { percentage: percent })}
      className="flex items-center gap-1.5 text-xs tabular-nums whitespace-nowrap text-ink-3"
    >
      <ProgressRing size={14} value={percent / 100} />
      {percent}%
    </span>
  ) : (
    <span className="flex items-center gap-1.5 text-xs whitespace-nowrap text-ink-3">
      {LANGUAGE_DETECTOR_SIZE}
      <DownloadMark accent />
    </span>
  );
  const stop = t("settings.general.language.stopDetectorDownload");

  return (
    <span className="flex items-center gap-1">
      {hint ? <Tooltip text={hint}>{state}</Tooltip> : state}
      {downloading && onCancel && (
        <button
          type="button"
          onClick={onCancel}
          title={stop}
          aria-label={stop}
          className="grid size-[20px] cursor-pointer place-items-center rounded-[6px] text-ink-3 hover:bg-line hover:text-text focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
        >
          <X className="size-[13px]" aria-hidden="true" />
        </button>
      )}
    </span>
  );
};
