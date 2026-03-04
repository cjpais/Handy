import React, { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { AudioPlayer } from "../../ui/AudioPlayer";
import { Button } from "../../ui/Button";
import {
  AlertCircle,
  Copy,
  Star,
  Check,
  Trash2,
  FolderOpen,
  Loader2,
  X,
} from "lucide-react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { readFile } from "@tauri-apps/plugin-fs";
import { commands, type HistoryEntry } from "@/bindings";
import { formatDateTime } from "@/utils/dateFormat";
import { useOsType } from "@/hooks/useOsType";
import { useFileImportStore, STAGE_I18N_KEYS } from "@/stores/fileImportStore";

interface OpenRecordingsButtonProps {
  onClick: () => void;
  label: string;
}

const OpenRecordingsButton: React.FC<OpenRecordingsButtonProps> = ({
  onClick,
  label,
}) => (
  <Button
    onClick={onClick}
    variant="secondary"
    size="sm"
    className="flex items-center gap-2"
    title={label}
  >
    <FolderOpen className="w-4 h-4" />
    <span>{label}</span>
  </Button>
);

export const HistorySettings: React.FC = () => {
  const { t } = useTranslation();
  const osType = useOsType();
  const [historyEntries, setHistoryEntries] = useState<HistoryEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const importIsRunning = useFileImportStore((state) => state.isRunning);
  const importFileName = useFileImportStore((state) => state.fileName);
  const importStage = useFileImportStore((state) => state.stage);
  const importMessage = useFileImportStore((state) => state.message);
  const importPercent = useFileImportStore((state) => state.percent);
  const importError = useFileImportStore((state) => state.error);
  const importSourceSampleRate = useFileImportStore(
    (state) => state.sourceSampleRate,
  );
  const importSampleRate = useFileImportStore((state) => state.sampleRate);
  const importChannels = useFileImportStore((state) => state.channels);
  const importDurationSec = useFileImportStore((state) => state.durationSec);
  const importBitrateKbps = useFileImportStore((state) => state.bitrateKbps);
  const clearImportError = useFileImportStore((state) => state.clearError);

  const loadHistoryEntries = useCallback(async () => {
    try {
      const result = await commands.getHistoryEntries();
      if (result.status === "ok") {
        setHistoryEntries(result.data);
      }
    } catch (error) {
      console.error("Failed to load history entries:", error);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadHistoryEntries();

    // Listen for history update events
    const setupListener = async () => {
      const unlisten = await listen("history-updated", () => {
        console.log("History updated, reloading entries...");
        loadHistoryEntries();
      });

      // Return cleanup function
      return unlisten;
    };

    let unlistenPromise = setupListener();

    return () => {
      unlistenPromise.then((unlisten) => {
        if (unlisten) {
          unlisten();
        }
      });
    };
  }, [loadHistoryEntries]);

  const toggleSaved = async (id: number) => {
    try {
      await commands.toggleHistoryEntrySaved(id);
      // No need to reload here - the event listener will handle it
    } catch (error) {
      console.error("Failed to toggle saved status:", error);
    }
  };

  const copyToClipboard = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
    } catch (error) {
      console.error("Failed to copy to clipboard:", error);
    }
  };

  const getAudioUrl = useCallback(
    async (fileName: string) => {
      try {
        const result = await commands.getAudioFilePath(fileName);
        if (result.status === "ok") {
          if (osType === "linux") {
            const fileData = await readFile(result.data);
            const blob = new Blob([fileData], { type: "audio/wav" });

            return URL.createObjectURL(blob);
          }

          return convertFileSrc(result.data, "asset");
        }
        return null;
      } catch (error) {
        console.error("Failed to get audio file path:", error);
        return null;
      }
    },
    [osType],
  );

  const deleteAudioEntry = async (id: number) => {
    try {
      await commands.deleteHistoryEntry(id);
    } catch (error) {
      console.error("Failed to delete audio entry:", error);
      throw error;
    }
  };

  const openRecordingsFolder = async () => {
    try {
      await commands.openRecordingsFolder();
    } catch (error) {
      console.error("Failed to open recordings folder:", error);
    }
  };

  const showImportBanner = importIsRunning || !!importError;
  const importFileLabel =
    importFileName || t("settings.history.import.audioFileFallback");
  const stageI18nKey = STAGE_I18N_KEYS[importStage || ""];
  const importStageMessage = stageI18nKey
    ? t(stageI18nKey)
    : importMessage || t("toasts.fileImport.processing");
  const importAudioMetaParts: string[] = [];
  if (importBitrateKbps != null) {
    importAudioMetaParts.push(`${importBitrateKbps} kbps`);
  }
  if (importSourceSampleRate) {
    const khz = importSourceSampleRate / 1000;
    const khzStr = Number.isInteger(khz) ? `${khz}` : khz.toFixed(1);
    importAudioMetaParts.push(`${khzStr} kHz`);
  }
  if (importChannels) {
    importAudioMetaParts.push(importChannels === 1 ? "mono" : "stereo");
  }
  if (importDurationSec != null) {
    const totalSec = Math.round(importDurationSec);
    const h = Math.floor(totalSec / 3600);
    const m = Math.floor((totalSec % 3600) / 60);
    const s = totalSec % 60;
    const duration =
      h > 0
        ? `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`
        : `${m}:${String(s).padStart(2, "0")}`;
    importAudioMetaParts.push(duration);
  }
  const importAudioMeta = importAudioMetaParts.join(" · ");

  const renderImportBanner = () => (
    <div className="bg-background border border-mid-gray/20 rounded-lg px-4 py-3">
      {importIsRunning ? (
        <div className="space-y-2">
          <div className="flex items-center gap-2 text-sm font-medium">
            <Loader2 className="w-4 h-4 animate-spin text-logo-primary" />
            <span>
              {t("settings.history.import.inProgress", {
                file: importFileLabel,
              })}
            </span>
            {importAudioMeta && (
              <span className="text-xs font-normal text-text/65">
                {importAudioMeta}
              </span>
            )}
          </div>
          <p className="text-xs text-text/70">{importStageMessage}</p>
          <div className="h-2 w-full rounded-full bg-mid-gray/20 overflow-hidden">
            <div
              className="h-full bg-logo-primary transition-all duration-200"
              style={{ width: `${importPercent}%` }}
            />
          </div>
          <p className="text-xs text-text/60">{importPercent}%</p>
        </div>
      ) : (
        <div className="flex items-start justify-between gap-3">
          <div className="flex items-start gap-2">
            <AlertCircle className="w-4 h-4 mt-0.5 text-red-500" />
            <div className="space-y-1">
              <p className="text-sm font-medium">
                {t("settings.history.import.failedTitle")}
              </p>
              <p className="text-xs text-text/70">
                {importError || t("settings.history.import.failedDescription")}
              </p>
            </div>
          </div>
          <button
            type="button"
            className="text-text/50 hover:text-text transition-colors"
            onClick={clearImportError}
            title={t("accessibility.dismiss")}
          >
            <X className="w-4 h-4" />
          </button>
        </div>
      )}
    </div>
  );

  if (loading) {
    return (
      <div className="max-w-3xl w-full mx-auto space-y-6">
        <div className="space-y-2">
          <div className="px-4 flex items-center justify-between">
            <div>
              <h2 className="text-xs font-medium text-mid-gray uppercase tracking-wide">
                {t("settings.history.title")}
              </h2>
            </div>
            <OpenRecordingsButton
              onClick={openRecordingsFolder}
              label={t("settings.history.openFolder")}
            />
          </div>
          {showImportBanner && renderImportBanner()}
          <div className="bg-background border border-mid-gray/20 rounded-lg overflow-visible">
            <div className="px-4 py-3 text-center text-text/60">
              {t("settings.history.loading")}
            </div>
          </div>
        </div>
      </div>
    );
  }

  if (historyEntries.length === 0) {
    return (
      <div className="max-w-3xl w-full mx-auto space-y-6">
        <div className="space-y-2">
          <div className="px-4 flex items-center justify-between">
            <div>
              <h2 className="text-xs font-medium text-mid-gray uppercase tracking-wide">
                {t("settings.history.title")}
              </h2>
            </div>
            <OpenRecordingsButton
              onClick={openRecordingsFolder}
              label={t("settings.history.openFolder")}
            />
          </div>
          {showImportBanner && renderImportBanner()}
          <div className="bg-background border border-mid-gray/20 rounded-lg overflow-visible">
            <div className="px-4 py-3 text-center text-text/60">
              {t("settings.history.empty")}
            </div>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <div className="space-y-2">
        <div className="px-4 flex items-center justify-between">
          <div>
            <h2 className="text-xs font-medium text-mid-gray uppercase tracking-wide">
              {t("settings.history.title")}
            </h2>
          </div>
          <OpenRecordingsButton
            onClick={openRecordingsFolder}
            label={t("settings.history.openFolder")}
          />
        </div>
        {showImportBanner && renderImportBanner()}
        <div className="bg-background border border-mid-gray/20 rounded-lg overflow-visible">
          <div className="divide-y divide-mid-gray/20">
            {historyEntries.map((entry) => (
              <HistoryEntryComponent
                key={entry.id}
                entry={entry}
                onToggleSaved={() => toggleSaved(entry.id)}
                onCopyText={() => copyToClipboard(entry.transcription_text)}
                getAudioUrl={getAudioUrl}
                deleteAudio={deleteAudioEntry}
              />
            ))}
          </div>
        </div>
      </div>
    </div>
  );
};

interface HistoryEntryProps {
  entry: HistoryEntry;
  onToggleSaved: () => void;
  onCopyText: () => void;
  getAudioUrl: (fileName: string) => Promise<string | null>;
  deleteAudio: (id: number) => Promise<void>;
}

const HistoryEntryComponent: React.FC<HistoryEntryProps> = ({
  entry,
  onToggleSaved,
  onCopyText,
  getAudioUrl,
  deleteAudio,
}) => {
  const { t, i18n } = useTranslation();
  const [showCopied, setShowCopied] = useState(false);

  const handleLoadAudio = useCallback(
    () => getAudioUrl(entry.file_name),
    [getAudioUrl, entry.file_name],
  );

  const handleCopyText = () => {
    onCopyText();
    setShowCopied(true);
    setTimeout(() => setShowCopied(false), 2000);
  };

  const handleDeleteEntry = async () => {
    try {
      await deleteAudio(entry.id);
    } catch (error) {
      console.error("Failed to delete entry:", error);
      alert("Failed to delete entry. Please try again.");
    }
  };

  const formattedDate = formatDateTime(String(entry.timestamp), i18n.language);

  return (
    <div className="px-4 py-2 pb-5 flex flex-col gap-3">
      <div className="flex justify-between items-center">
        <p className="text-sm font-medium">{formattedDate}</p>
        <div className="flex items-center gap-1">
          <button
            onClick={handleCopyText}
            className="text-text/50 hover:text-logo-primary  hover:border-logo-primary transition-colors cursor-pointer"
            title={t("settings.history.copyToClipboard")}
          >
            {showCopied ? (
              <Check width={16} height={16} />
            ) : (
              <Copy width={16} height={16} />
            )}
          </button>
          <button
            onClick={onToggleSaved}
            className={`p-2 rounded-md transition-colors cursor-pointer ${
              entry.saved
                ? "text-logo-primary hover:text-logo-primary/80"
                : "text-text/50 hover:text-logo-primary"
            }`}
            title={
              entry.saved
                ? t("settings.history.unsave")
                : t("settings.history.save")
            }
          >
            <Star
              width={16}
              height={16}
              fill={entry.saved ? "currentColor" : "none"}
            />
          </button>
          <button
            onClick={handleDeleteEntry}
            className="text-text/50 hover:text-logo-primary transition-colors cursor-pointer"
            title={t("settings.history.delete")}
          >
            <Trash2 width={16} height={16} />
          </button>
        </div>
      </div>
      <p className="italic text-text/90 text-sm pb-2 select-text cursor-text">
        {entry.transcription_text}
      </p>
      <AudioPlayer onLoadRequest={handleLoadAudio} className="w-full" />
    </div>
  );
};
