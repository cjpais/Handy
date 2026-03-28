import React, { useCallback, useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { readFile } from "@tauri-apps/plugin-fs";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Check,
  Copy,
  FileText,
  FolderOpen,
  Loader2,
  Mic,
  RotateCcw,
  Star,
  Trash2,
  Upload,
  X,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  commands,
  events,
  type HistoryEntry,
  type HistoryUpdatePayload,
} from "@/bindings";

interface ImportProgress {
  stage: string;
  percent: number;
  message: string;
}
import { useOsType } from "@/hooks/useOsType";
import { formatDateTime } from "@/utils/dateFormat";
import { AudioPlayer } from "../../ui/AudioPlayer";
import { Button } from "../../ui/Button";
import { ExportOptions } from "./ExportOptions";

const IconButton: React.FC<{
  onClick: () => void;
  title: string;
  disabled?: boolean;
  active?: boolean;
  children: React.ReactNode;
}> = ({ onClick, title, disabled, active, children }) => (
  <button
    onClick={onClick}
    disabled={disabled}
    className={`p-1.5 rounded-md flex items-center justify-center transition-colors cursor-pointer disabled:cursor-not-allowed disabled:text-text/20 ${
      active
        ? "text-logo-primary hover:text-logo-primary/80"
        : "text-text/50 hover:text-logo-primary"
    }`}
    title={title}
  >
    {children}
  </button>
);

const PAGE_SIZE = 30;

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

function formatDuration(seconds: number): string {
  const minutes = Math.floor(seconds / 60);
  const remainingSeconds = Math.floor(seconds % 60);
  return `${minutes}:${remainingSeconds.toString().padStart(2, "0")}`;
}

export const HistorySettings: React.FC = () => {
  const { t } = useTranslation();
  const osType = useOsType();
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [hasMore, setHasMore] = useState(true);
  const sentinelRef = useRef<HTMLDivElement>(null);
  const entriesRef = useRef<HistoryEntry[]>([]);
  const loadingRef = useRef(false);

  // Import state
  const [isImporting, setIsImporting] = useState(false);
  const [importProgress, setImportProgress] = useState<ImportProgress | null>(
    null,
  );

  // Search and filter state
  const [searchQuery, setSearchQuery] = useState("");
  const [filterSource, setFilterSource] = useState<
    "all" | "recording" | "upload"
  >("all");

  // Keep ref in sync for use in IntersectionObserver callback
  useEffect(() => {
    entriesRef.current = entries;
  }, [entries]);

  const loadPage = useCallback(async (cursor?: number) => {
    const isFirstPage = cursor === undefined;
    if (!isFirstPage && loadingRef.current) return;
    loadingRef.current = true;

    if (isFirstPage) setLoading(true);

    try {
      const result = await commands.getHistoryEntries(
        cursor ?? null,
        PAGE_SIZE,
      );
      if (result.status === "ok") {
        const { entries: newEntries, has_more } = result.data;
        setEntries((prev) =>
          isFirstPage ? newEntries : [...prev, ...newEntries],
        );
        setHasMore(has_more);
      }
    } catch (error) {
      console.error("Failed to load history entries:", error);
    } finally {
      setLoading(false);
      loadingRef.current = false;
    }
  }, []);

  // Initial load
  useEffect(() => {
    loadPage();
  }, [loadPage]);

  // Infinite scroll via IntersectionObserver
  useEffect(() => {
    if (loading) return;

    const sentinel = sentinelRef.current;
    if (!sentinel || !hasMore) return;

    const observer = new IntersectionObserver(
      (observerEntries) => {
        const first = observerEntries[0];
        if (first.isIntersecting) {
          const lastEntry = entriesRef.current[entriesRef.current.length - 1];
          if (lastEntry) {
            loadPage(lastEntry.id);
          }
        }
      },
      { threshold: 0 },
    );

    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [loading, hasMore, loadPage]);

  // Listen for history update events (from transcription pipeline AND import)
  useEffect(() => {
    const unlisten = events.historyUpdatePayload.listen((event) => {
      const payload: HistoryUpdatePayload = event.payload;
      if (payload.action === "added") {
        setEntries((prev) => [payload.entry, ...prev]);
      } else if (payload.action === "updated") {
        setEntries((prev) =>
          prev.map((e) => (e.id === payload.entry.id ? payload.entry : e)),
        );
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  // Listen for import progress events
  useEffect(() => {
    const setupListener = async () => {
      const unlistenProgress = await listen<ImportProgress>(
        "import-progress",
        (event) => {
          const progress = event.payload;
          setImportProgress(progress);

          if (
            progress.stage === "completed" ||
            progress.stage === "failed" ||
            progress.stage === "cancelled"
          ) {
            // Clear import state after a short delay
            setTimeout(() => {
              setIsImporting(false);
              setImportProgress(null);
            }, 1500);
          }
        },
      );

      return unlistenProgress;
    };

    const unlistenPromise = setupListener();

    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  // Filter and search logic (client-side on loaded entries)
  const filteredEntries = entries.filter((entry) => {
    const matchesSearch =
      searchQuery === "" ||
      entry.title.toLowerCase().includes(searchQuery.toLowerCase()) ||
      entry.transcription_text
        .toLowerCase()
        .includes(searchQuery.toLowerCase());

    const matchesSource =
      filterSource === "all" ||
      (filterSource === "recording" && entry.source !== "upload") ||
      (filterSource === "upload" && entry.source === "upload");

    return matchesSearch && matchesSource;
  });

  const toggleSaved = async (id: number) => {
    setEntries((prev) =>
      prev.map((e) => (e.id === id ? { ...e, saved: !e.saved } : e)),
    );
    try {
      const result = await commands.toggleHistoryEntrySaved(id);
      if (result.status !== "ok") {
        setEntries((prev) =>
          prev.map((e) => (e.id === id ? { ...e, saved: !e.saved } : e)),
        );
      }
    } catch (error) {
      console.error("Failed to toggle saved status:", error);
      setEntries((prev) =>
        prev.map((e) => (e.id === id ? { ...e, saved: !e.saved } : e)),
      );
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
    setEntries((prev) => prev.filter((e) => e.id !== id));
    try {
      const result = await commands.deleteHistoryEntry(id);
      if (result.status !== "ok") {
        loadPage();
      }
    } catch (error) {
      console.error("Failed to delete entry:", error);
      loadPage();
    }
  };

  const retryHistoryEntry = async (id: number) => {
    const result = await commands.retryHistoryEntryTranscription(id);
    if (result.status !== "ok") {
      throw new Error(String(result.error));
    }
  };

  const openRecordingsFolder = async () => {
    try {
      const result = await commands.openRecordingsFolder();
      if (result.status !== "ok") {
        throw new Error(String(result.error));
      }
    } catch (error) {
      console.error("Failed to open recordings folder:", error);
    }
  };

  const handleCancelImport = async () => {
    try {
      // Cancel with empty string - the backend cancels all active imports
      await commands.cancelImport("");
      toast.info(t("settings.history.cancellingImport"));
    } catch (error) {
      console.error("Failed to cancel import:", error);
    }
  };

  const handleImport = async () => {
    try {
      const selected = await open({
        multiple: false,
        filters: [
          {
            name: "Audio",
            extensions: [
              "mp3",
              "m4a",
              "wav",
              "ogg",
              "flac",
              "aac",
              "wma",
              "aiff",
            ],
          },
        ],
      });

      if (selected) {
        setIsImporting(true);
        setImportProgress(null);
        toast.info(t("settings.history.importing"));

        const result = await commands.importAudioFile(selected as string);
        if (result.status === "ok") {
          toast.success(t("settings.history.importSuccess"));
        } else {
          toast.error(
            `${t("settings.history.importFailed")}: ${result.error}`,
          );
        }
      }
    } catch (error) {
      console.error("Failed to import audio:", error);
      toast.error(`${t("settings.history.importFailed")}: ${error}`);
    } finally {
      setIsImporting(false);
      setImportProgress(null);
    }
  };

  // Determine content to render
  const showFiltered = searchQuery !== "" || filterSource !== "all";
  const displayEntries = showFiltered ? filteredEntries : entries;

  let content: React.ReactNode;

  if (loading) {
    content = (
      <div className="px-4 py-3 text-center text-text/60">
        {t("settings.history.loading")}
      </div>
    );
  } else if (displayEntries.length === 0) {
    content = (
      <div className="px-4 py-3 text-center text-text/60">
        {showFiltered
          ? t("settings.history.noMatchingEntries")
          : t("settings.history.empty")}
      </div>
    );
  } else {
    content = (
      <>
        <div className="divide-y divide-mid-gray/20">
          {displayEntries.map((entry) => (
            <HistoryEntryComponent
              key={entry.id}
              entry={entry}
              onToggleSaved={() => toggleSaved(entry.id)}
              onCopyText={() => copyToClipboard(entry.transcription_text)}
              getAudioUrl={getAudioUrl}
              deleteAudio={deleteAudioEntry}
              retryTranscription={retryHistoryEntry}
            />
          ))}
        </div>
        {/* Sentinel for infinite scroll */}
        {!showFiltered && <div ref={sentinelRef} className="h-1" />}
      </>
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
          <div className="flex gap-2">
            <Button
              onClick={handleImport}
              variant="secondary"
              size="sm"
              className="flex items-center gap-2"
              disabled={isImporting}
            >
              {isImporting ? (
                <Loader2 className="w-4 h-4 animate-spin" />
              ) : (
                <Upload className="w-4 h-4" />
              )}
              <span>
                {isImporting
                  ? importProgress?.message || t("settings.history.importing")
                  : t("settings.history.importAudio")}
              </span>
            </Button>
            <OpenRecordingsButton
              onClick={openRecordingsFolder}
              label={t("settings.history.openFolder")}
            />
          </div>
        </div>

        {/* Search and Filter Section */}
        <div className="px-4 space-y-3">
          {/* Search box */}
          <div className="flex items-center gap-2">
            <input
              type="text"
              placeholder={t("settings.history.searchPlaceholder")}
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="flex-1 px-3 py-1.5 text-sm bg-background border border-mid-gray/20 rounded text-text placeholder:text-text/40 focus:outline-none focus:border-logo-primary"
            />
            {searchQuery && (
              <button
                onClick={() => setSearchQuery("")}
                className="text-mid-gray hover:text-text transition-colors"
                title="Clear search"
              >
                <X className="w-4 h-4" />
              </button>
            )}
          </div>

          {/* Filter buttons */}
          <div className="flex gap-2">
            {(["all", "recording", "upload"] as const).map((source) => (
              <button
                key={source}
                onClick={() => setFilterSource(source)}
                className={`px-3 py-1 rounded text-xs transition-colors ${
                  filterSource === source
                    ? "bg-logo-primary text-white"
                    : "bg-mid-gray/10 text-text hover:bg-mid-gray/20"
                }`}
              >
                {source === "all" && t("settings.history.filterAll")}
                {source === "recording" &&
                  `🎤 ${t("settings.history.filterRecordings")}`}
                {source === "upload" &&
                  `📁 ${t("settings.history.filterUploaded")}`}
              </button>
            ))}
          </div>

          {/* Results count */}
          {showFiltered && (
            <div className="text-xs text-mid-gray">
              {t("settings.history.foundEntries", {
                found: filteredEntries.length,
                total: entries.length,
              })}
            </div>
          )}
        </div>

        {/* Import Progress Section */}
        {isImporting && importProgress && (
          <div className="px-4 space-y-2">
            <div className="flex items-center justify-between text-xs text-mid-gray">
              <span>
                {importProgress.stage === "decoding" &&
                  `🔄 ${t("settings.history.importDecoding")}`}
                {importProgress.stage === "transcribing" &&
                  `🎤 ${t("settings.history.importTranscribing")}`}
                {importProgress.stage === "saving" &&
                  `💾 ${t("settings.history.importSaving")}`}
                {importProgress.stage === "starting" &&
                  `🚀 ${t("settings.history.importStarting")}`}
                {importProgress.stage === "completed" &&
                  `✅ ${t("settings.history.importCompleted")}`}
                {importProgress.stage === "failed" &&
                  `❌ ${t("settings.history.importFailed")}`}
                {importProgress.stage === "cancelled" &&
                  `🚫 ${t("settings.history.importCancelled")}`}
                {!["decoding", "transcribing", "saving", "starting", "completed", "failed", "cancelled"].includes(importProgress.stage) &&
                  importProgress.message}
              </span>
              <Button
                onClick={handleCancelImport}
                variant="secondary"
                size="sm"
                className="h-6 px-2 text-xs"
              >
                {t("settings.history.importCancel")}
              </Button>
            </div>
            <div className="w-full h-1 bg-mid-gray/10 rounded-full overflow-hidden">
              <div
                className="h-full bg-logo-primary transition-all duration-300 ease-out"
                style={{ width: `${importProgress.percent || 0}%` }}
              />
            </div>
          </div>
        )}

        {/* History List */}
        <div className="bg-background border border-mid-gray/20 rounded-lg overflow-visible">
          {content}
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
  retryTranscription: (id: number) => Promise<void>;
}

const HistoryEntryComponent: React.FC<HistoryEntryProps> = ({
  entry,
  onToggleSaved,
  onCopyText,
  getAudioUrl,
  deleteAudio,
  retryTranscription,
}) => {
  const { t, i18n } = useTranslation();
  const [showCopied, setShowCopied] = useState(false);
  const [retrying, setRetrying] = useState(false);

  const hasTranscription = entry.transcription_text.trim().length > 0;

  const handleLoadAudio = useCallback(
    () => getAudioUrl(entry.file_name),
    [getAudioUrl, entry.file_name],
  );

  const handleCopyText = () => {
    if (!hasTranscription) {
      return;
    }

    onCopyText();
    setShowCopied(true);
    setTimeout(() => setShowCopied(false), 2000);
  };

  const handleDeleteEntry = async () => {
    try {
      await deleteAudio(entry.id);
    } catch (error) {
      console.error("Failed to delete entry:", error);
      toast.error(t("settings.history.deleteError"));
    }
  };

  const handleRetranscribe = async () => {
    try {
      setRetrying(true);
      await retryTranscription(entry.id);
    } catch (error) {
      console.error("Failed to re-transcribe:", error);
      toast.error(t("settings.history.retranscribeError"));
    } finally {
      setRetrying(false);
    }
  };

  const formattedDate = formatDateTime(String(entry.timestamp), i18n.language);

  return (
    <div className="px-4 py-2 pb-5 flex flex-col gap-3">
      <div className="flex justify-between items-center">
        <div className="flex items-center gap-2">
          {/* Source icon */}
          {entry.source === "upload" ? (
            <span title={t("settings.history.sourceUpload")}>
              <FileText className="w-4 h-4 text-mid-gray" />
            </span>
          ) : (
            <span title={t("settings.history.sourceRecording")}>
              <Mic className="w-4 h-4 text-mid-gray" />
            </span>
          )}
          <p className="text-sm font-medium">{formattedDate}</p>
          {/* Duration badge */}
          {entry.duration != null && (
            <span className="text-xs text-mid-gray bg-mid-gray/10 px-1.5 py-0.5 rounded">
              {formatDuration(entry.duration)}
            </span>
          )}
        </div>
        <div className="flex items-center">
          <ExportOptions entry={entry} />
          <IconButton
            onClick={handleCopyText}
            disabled={!hasTranscription || retrying}
            title={t("settings.history.copyToClipboard")}
          >
            {showCopied ? (
              <Check width={16} height={16} />
            ) : (
              <Copy width={16} height={16} />
            )}
          </IconButton>
          <IconButton
            onClick={onToggleSaved}
            disabled={retrying}
            active={entry.saved}
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
          </IconButton>
          <IconButton
            onClick={handleRetranscribe}
            disabled={retrying}
            title={t("settings.history.retranscribe")}
          >
            <RotateCcw
              width={16}
              height={16}
              style={
                retrying
                  ? { animation: "spin 1s linear infinite reverse" }
                  : undefined
              }
            />
          </IconButton>
          <IconButton
            onClick={handleDeleteEntry}
            disabled={retrying}
            title={t("settings.history.delete")}
          >
            <Trash2 width={16} height={16} />
          </IconButton>
        </div>
      </div>

      <p
        className={`italic text-sm pb-2 ${
          retrying
            ? ""
            : hasTranscription
              ? "text-text/90 select-text cursor-text whitespace-pre-wrap break-words"
              : "text-text/40"
        }`}
        style={
          retrying
            ? { animation: "transcribe-pulse 3s ease-in-out infinite" }
            : undefined
        }
      >
        {retrying && (
          <style>{`
            @keyframes transcribe-pulse {
              0%, 100% { color: color-mix(in srgb, var(--color-text) 40%, transparent); }
              50% { color: color-mix(in srgb, var(--color-text) 90%, transparent); }
            }
          `}</style>
        )}
        {retrying
          ? t("settings.history.transcribing")
          : hasTranscription
            ? entry.transcription_text
            : t("settings.history.transcriptionFailed")}
      </p>

      <AudioPlayer onLoadRequest={handleLoadAudio} className="w-full" />
    </div>
  );
};
