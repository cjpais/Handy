import React, { useCallback, useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { readFile } from "@tauri-apps/plugin-fs";
import {
  AlertCircle,
  Check,
  ChevronRight,
  Copy,
  FolderOpen,
  RotateCcw,
  Sparkles,
  Square,
  Star,
  Trash2,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  commands,
  events,
  type HistoryEntry,
  type HistoryUpdatePayload,
} from "@/bindings";
import { useOsType } from "@/hooks/useOsType";
import { formatDateTime } from "@/utils/dateFormat";
import { AudioPlayer } from "../../ui/AudioPlayer";
import { Button } from "../../ui/Button";

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

export const HistorySettings: React.FC = () => {
  const { t } = useTranslation();
  const osType = useOsType();
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [hasMore, setHasMore] = useState(true);
  const sentinelRef = useRef<HTMLDivElement>(null);
  const entriesRef = useRef<HistoryEntry[]>([]);
  const loadingRef = useRef(false);

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

  // Listen for new entries added from the transcription pipeline
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
      // "deleted" and "toggled" are handled by optimistic updates only,
      // so we intentionally ignore them here to avoid double-mutation.
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const toggleSaved = async (id: number) => {
    // Optimistic update
    setEntries((prev) =>
      prev.map((e) => (e.id === id ? { ...e, saved: !e.saved } : e)),
    );
    try {
      const result = await commands.toggleHistoryEntrySaved(id);
      if (result.status !== "ok") {
        // Revert on failure
        setEntries((prev) =>
          prev.map((e) => (e.id === id ? { ...e, saved: !e.saved } : e)),
        );
      }
    } catch (error) {
      console.error("Failed to toggle saved status:", error);
      // Revert on failure
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
    // Optimistically remove
    setEntries((prev) => prev.filter((e) => e.id !== id));
    try {
      const result = await commands.deleteHistoryEntry(id);
      if (result.status !== "ok") {
        // Reload on failure
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

  const summarizeEntry = async (id: number) => {
    const result = await commands.summarizeHistoryEntry(id);
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

  let content: React.ReactNode;

  if (loading) {
    content = (
      <div className="px-4 py-3 text-center text-text/60">
        {t("settings.history.loading")}
      </div>
    );
  } else if (entries.length === 0) {
    content = (
      <div className="px-4 py-3 text-center text-text/60">
        {t("settings.history.empty")}
      </div>
    );
  } else {
    content = (
      <>
        <div className="divide-y divide-mid-gray/20">
          {entries.map((entry) => (
            <HistoryEntryComponent
              key={entry.id}
              entry={entry}
              onToggleSaved={() => toggleSaved(entry.id)}
              copyText={copyToClipboard}
              getAudioUrl={getAudioUrl}
              deleteAudio={deleteAudioEntry}
              retryTranscription={retryHistoryEntry}
              summarizeEntry={summarizeEntry}
            />
          ))}
        </div>
        {/* Sentinel for infinite scroll */}
        <div ref={sentinelRef} className="h-1" />
      </>
    );
  }

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <div className="space-y-2">
        <div className="px-4 flex items-center justify-end">
          <OpenRecordingsButton
            onClick={openRecordingsFolder}
            label={t("entries.openFolder")}
          />
        </div>
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
  copyText: (text: string) => Promise<void>;
  getAudioUrl: (fileName: string) => Promise<string | null>;
  deleteAudio: (id: number) => Promise<void>;
  retryTranscription: (id: number) => Promise<void>;
  summarizeEntry: (id: number) => Promise<void>;
}

const HistoryEntryComponent: React.FC<HistoryEntryProps> = ({
  entry,
  onToggleSaved,
  copyText,
  getAudioUrl,
  deleteAudio,
  retryTranscription,
  summarizeEntry,
}) => {
  const { t, i18n } = useTranslation();
  const [showCopied, setShowCopied] = useState(false);
  const [retrying, setRetrying] = useState(false);
  const [summarizing, setSummarizing] = useState(false);
  const [detailsOpen, setDetailsOpen] = useState(false);

  const hasTranscription = entry.transcription_text.trim().length > 0;
  const hasPostProcessed = Boolean(entry.post_processed_text?.trim());
  const hasSummary = Boolean(entry.summary?.trim());
  const summaryPending = entry.summary_status === "pending" || summarizing;
  const summaryFailed = entry.summary_status === "failed" && !summarizing;
  const hasActions = entry.actions.length > 0;

  // The best-available cleaned text, used as the evidence/fallback body.
  const bodyText = entry.post_processed_text || entry.transcription_text;
  const hasBody = Boolean(bodyText?.trim());

  const handleLoadAudio = useCallback(
    () => getAudioUrl(entry.file_name),
    [getAudioUrl, entry.file_name],
  );

  const derivedTitle = (() => {
    if (entry.summary_title?.trim()) return entry.summary_title.trim();
    const source = entry.summary || bodyText;
    if (!source?.trim()) return entry.title;
    const firstSentence = source.match(/^[^.!?\n]+[.!?]?/)?.[0] ?? source;
    return firstSentence.length > 72
      ? firstSentence.slice(0, 69).trimEnd() + "…"
      : firstSentence.trim();
  })();

  const handleCopyText = async () => {
    const textToCopy = hasSummary ? entry.summary! : bodyText;
    if (!textToCopy.trim() || retrying) return;
    await copyText(textToCopy);
    setShowCopied(true);
    setTimeout(() => setShowCopied(false), 2000);
  };

  const handleSummarize = async () => {
    try {
      setSummarizing(true);
      await summarizeEntry(entry.id);
    } catch (error) {
      console.error("Failed to summarise:", error);
      toast.error(t("settings.history.summarizeError"));
    } finally {
      setSummarizing(false);
    }
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
  const canCopy = (hasSummary || hasBody || hasTranscription) && !retrying;

  return (
    <div className="px-4 py-4 flex flex-col gap-2">
      {/* Title + tools */}
      <div className="flex justify-between items-start gap-2">
        <p className="text-sm font-semibold text-text leading-snug">
          {derivedTitle || t("entries.untitled")}
        </p>
        <div className="flex items-center shrink-0 -mt-1 -mr-1.5">
          <IconButton
            onClick={handleCopyText}
            disabled={!canCopy}
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

      {/* Metadata */}
      <p className="text-xs text-text/50">{formattedDate}</p>

      {/* Main output */}
      <div className="mt-1">
        {retrying ? (
          <>
            <style>{`
              @keyframes transcribe-pulse {
                0%, 100% { color: color-mix(in srgb, var(--color-text) 40%, transparent); }
                50% { color: color-mix(in srgb, var(--color-text) 90%, transparent); }
              }
            `}</style>
            <p
              className="text-sm italic"
              style={{ animation: "transcribe-pulse 3s ease-in-out infinite" }}
            >
              {t("settings.history.transcribing")}
            </p>
          </>
        ) : hasSummary ? (
          <p className="text-sm text-text/90 select-text cursor-text whitespace-pre-wrap break-words">
            {entry.summary}
          </p>
        ) : summaryPending ? (
          <>
            <style>{`
              @keyframes summarize-pulse {
                0%, 100% { color: color-mix(in srgb, var(--color-text) 40%, transparent); }
                50% { color: color-mix(in srgb, var(--color-text) 90%, transparent); }
              }
            `}</style>
            <p
              className="text-sm italic flex items-center gap-1.5"
              style={{ animation: "summarize-pulse 3s ease-in-out infinite" }}
            >
              <Sparkles width={14} height={14} />
              {t("settings.history.summarizing")}
            </p>
          </>
        ) : hasBody ? (
          <p
            className={`text-sm select-text cursor-text whitespace-pre-wrap break-words ${
              hasPostProcessed ? "text-text/90" : "italic text-text/90"
            }`}
          >
            {bodyText}
          </p>
        ) : (
          <p className="text-sm italic text-text/40">
            {t("settings.history.transcriptionFailed")}
          </p>
        )}
      </div>

      {/* Actions checklist */}
      {hasActions && (
        <ul className="mt-1 flex flex-col gap-1.5">
          {entry.actions.map((action, index) => (
            <li key={index} className="flex items-start gap-2 text-sm">
              <Square
                width={14}
                height={14}
                className="mt-0.5 shrink-0 text-text/40"
              />
              <span className="select-text cursor-text">
                <span className="text-text/90">{action.description}</span>
                {action.assignee && (
                  <span className="ml-2 inline-block rounded bg-mid-gray/15 px-1.5 py-0.5 text-xs text-text/60">
                    {action.assignee}
                  </span>
                )}
                {action.due && (
                  <span className="ml-1.5 inline-block rounded bg-mid-gray/15 px-1.5 py-0.5 text-xs text-text/60">
                    {action.due}
                  </span>
                )}
              </span>
            </li>
          ))}
        </ul>
      )}

      {/* Summary failure / retry affordance */}
      {summaryFailed && (
        <button
          onClick={handleSummarize}
          className="mt-1 flex w-fit items-center gap-1.5 text-xs text-text/50 hover:text-logo-primary transition-colors cursor-pointer"
          title={t("settings.history.summarizeRetry")}
        >
          <AlertCircle width={13} height={13} />
          {t("settings.history.summarizeFailed")}
        </button>
      )}

      {/* Details accordion */}
      <details
        className="mt-2"
        onToggle={(e) => setDetailsOpen(e.currentTarget.open)}
      >
        <summary className="list-none flex items-center gap-1 text-xs text-text/40 hover:text-text/60 cursor-pointer select-none w-fit transition-colors">
          <ChevronRight
            width={12}
            height={12}
            className={`transition-transform ${detailsOpen ? "rotate-90" : ""}`}
          />
          {t("entries.viewDetails")}
        </summary>
        <div className="mt-3 flex flex-col gap-3 pt-3 border-t border-mid-gray/10">
          {hasSummary && hasPostProcessed && (
            <div className="flex flex-col gap-1.5">
              <p className="text-xs font-medium text-text/40 uppercase tracking-wide">
                {t("entries.cleanedText")}
              </p>
              <p className="text-xs text-text/70 select-text cursor-text whitespace-pre-wrap break-words">
                {entry.post_processed_text}
              </p>
            </div>
          )}
          {(hasSummary || hasPostProcessed) && hasTranscription && (
            <div className="flex flex-col gap-1.5">
              <p className="text-xs font-medium text-text/40 uppercase tracking-wide">
                {t("entries.transcript")}
              </p>
              <p className="text-xs italic text-text/70 select-text cursor-text whitespace-pre-wrap break-words">
                {entry.transcription_text}
              </p>
            </div>
          )}
          <AudioPlayer onLoadRequest={handleLoadAudio} className="w-full" />
        </div>
      </details>
    </div>
  );
};
