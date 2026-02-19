import React, { useState, useEffect, useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { AudioPlayer } from "../../ui/AudioPlayer";
import { Button } from "../../ui/Button";
import {
  Copy,
  Star,
  Check,
  Trash2,
  FolderOpen,
  Sparkles,
  Loader2,
} from "lucide-react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { readFile } from "@tauri-apps/plugin-fs";
import { commands, type HistoryEntry } from "@/bindings";
import { formatDateTime } from "@/utils/dateFormat";
import { useOsType } from "@/hooks/useOsType";
import { toast } from "sonner";
import { useSettings } from "@/hooks/useSettings";
import { VersionHistory } from "./VersionHistory";

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

  const { getSetting } = useSettings();
  const historyPostProcessEnabled =
    (getSetting("experimental_enabled") || false) &&
    (getSetting("post_process_enabled") || false) &&
    (getSetting("history_post_process_enabled") || false);

  const providerId = getSetting("post_process_provider_id");
  const selectedPromptId = getSetting("post_process_selected_prompt_id");
  const postProcessConfigured = useMemo(
    () => !!providerId && !!selectedPromptId,
    [providerId, selectedPromptId],
  );

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
        <div className="bg-background border border-mid-gray/20 rounded-lg overflow-visible">
          <div className="divide-y divide-mid-gray/20">
            {historyEntries.map((entry) => (
              <HistoryEntryComponent
                key={entry.id}
                entry={entry}
                onToggleSaved={() => toggleSaved(entry.id)}
                getAudioUrl={getAudioUrl}
                deleteAudio={deleteAudioEntry}
                showPostProcess={historyPostProcessEnabled}
                postProcessConfigured={postProcessConfigured}
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
  getAudioUrl: (fileName: string) => Promise<string | null>;
  deleteAudio: (id: number) => Promise<void>;
  showPostProcess: boolean;
  postProcessConfigured: boolean;
}

const HistoryEntryComponent: React.FC<HistoryEntryProps> = ({
  entry,
  onToggleSaved,
  getAudioUrl,
  deleteAudio,
  showPostProcess,
  postProcessConfigured,
}) => {
  const { t, i18n } = useTranslation();
  const [showCopied, setShowCopied] = useState(false);
  const [isProcessing, setIsProcessing] = useState(false);
  const [showOriginal, setShowOriginal] = useState(false);

  const hasEnhancedText = entry.post_processed_text != null;
  const displayText =
    hasEnhancedText && !showOriginal
      ? (entry.post_processed_text ?? entry.transcription_text)
      : entry.transcription_text;

  const handleLoadAudio = useCallback(
    () => getAudioUrl(entry.file_name),
    [getAudioUrl, entry.file_name],
  );

  const handleCopyText = () => {
    navigator.clipboard.writeText(displayText).catch((error) => {
      console.error("Failed to copy to clipboard:", error);
    });
    setShowCopied(true);
    setTimeout(() => setShowCopied(false), 2000);
  };

  const handleDeleteEntry = async () => {
    try {
      await deleteAudio(entry.id);
    } catch (error) {
      console.error("Failed to delete entry:", error);
      alert(t("settings.history.deleteError"));
    }
  };

  const handlePostProcess = async () => {
    setIsProcessing(true);
    try {
      const result = await commands.postProcessHistoryEntry(entry.id);
      if (result.status === "ok") {
        setShowOriginal(false);
      } else {
        const errorKey: Record<string, string> = {
          HISTORY_POST_PROCESS_DISABLED: "settings.history.postProcessDisabled",
          TRANSCRIPTION_EMPTY: "settings.history.postProcessEmptyText",
          POST_PROCESS_FAILED: "settings.history.postProcessError",
        };
        const key =
          errorKey[result.error] ?? "settings.history.postProcessError";
        toast.error(t(key));
      }
    } catch (error) {
      toast.error(t("settings.history.postProcessError"));
      console.error("Failed to post-process entry:", error);
    } finally {
      setIsProcessing(false);
    }
  };

  const formattedDate = formatDateTime(String(entry.timestamp), i18n.language);

  return (
    <div className="px-4 py-2 pb-5 flex flex-col gap-3">
      <div className="flex justify-between items-center">
        <p className="text-sm font-medium">{formattedDate}</p>
        {hasEnhancedText && (
          <button
            onClick={() => setShowOriginal(!showOriginal)}
            className="text-xs px-2 py-1 border border-text/20 rounded text-text/50 hover:text-logo-primary hover:border-logo-primary transition-colors cursor-pointer"
          >
            {showOriginal
              ? t("settings.history.showEnhanced")
              : t("settings.history.showOriginal")}
          </button>
        )}
        <div className="flex items-center gap-1">
          {showPostProcess && (
            <button
              onClick={handlePostProcess}
              disabled={
                isProcessing ||
                !postProcessConfigured ||
                entry.transcription_text.trim().length === 0
              }
              className="p-2 rounded-md text-text/50 hover:text-logo-primary transition-colors cursor-pointer disabled:opacity-30 disabled:cursor-not-allowed"
              title={
                postProcessConfigured
                  ? t("settings.history.postProcess")
                  : t("settings.history.postProcessNotConfigured")
              }
            >
              {isProcessing ? (
                <Loader2 width={16} height={16} className="animate-spin" />
              ) : (
                <Sparkles width={16} height={16} />
              )}
            </button>
          )}
          <button
            onClick={handleCopyText}
            className="text-text/50 hover:text-logo-primary hover:border-logo-primary transition-colors cursor-pointer"
            title={
              hasEnhancedText
                ? showOriginal
                  ? t("settings.history.copyOriginal")
                  : t("settings.history.copyEnhanced")
                : t("settings.history.copyToClipboard")
            }
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
        {displayText}
      </p>
      {entry.version_count > 0 && <VersionHistory entry={entry} />}
      <AudioPlayer onLoadRequest={handleLoadAudio} className="w-full" />
    </div>
  );
};
