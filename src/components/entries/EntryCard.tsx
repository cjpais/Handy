import { useCallback, useState } from "react";
import {
  AlertCircle,
  Check,
  ChevronRight,
  Copy,
  RotateCcw,
  Sparkles,
  Square,
  Star,
  Trash2,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { type HistoryEntry } from "@/bindings";
import { formatDateTime } from "@/utils/dateFormat";
import { AudioPlayer } from "../ui/AudioPlayer";

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

export interface EntryCardProps {
  entry: HistoryEntry;
  onToggleSaved: () => void;
  copyText: (text: string) => Promise<void>;
  getAudioUrl: (fileName: string) => Promise<string | null>;
  deleteAudio: (id: number) => Promise<void>;
  retryTranscription: (id: number) => Promise<void>;
  summarizeEntry: (id: number) => Promise<void>;
}

export const EntryCard: React.FC<EntryCardProps> = ({
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
      toast.error(t("entries.summarizeError"));
    } finally {
      setSummarizing(false);
    }
  };

  const handleDeleteEntry = async () => {
    try {
      await deleteAudio(entry.id);
    } catch (error) {
      console.error("Failed to delete entry:", error);
      toast.error(t("entries.deleteError"));
    }
  };

  const handleRetranscribe = async () => {
    try {
      setRetrying(true);
      await retryTranscription(entry.id);
    } catch (error) {
      console.error("Failed to re-transcribe:", error);
      toast.error(t("entries.retranscribeError"));
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
            title={t("entries.copyToClipboard")}
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
            title={entry.saved ? t("entries.unsave") : t("entries.save")}
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
            title={t("entries.retranscribe")}
          >
            <RotateCcw
              width={16}
              height={16}
              className={retrying ? "animate-spin-reverse" : undefined}
            />
          </IconButton>
          <IconButton
            onClick={handleDeleteEntry}
            disabled={retrying}
            title={t("entries.delete")}
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
          <p className="text-sm italic animate-transcribe-pulse">
            {t("entries.transcribing")}
          </p>
        ) : hasSummary ? (
          <p className="text-sm text-text/90 select-text cursor-text whitespace-pre-wrap break-words">
            {entry.summary}
          </p>
        ) : summaryPending ? (
          <p className="text-sm italic flex items-center gap-1.5 animate-summarize-pulse">
            <Sparkles width={14} height={14} />
            {t("entries.summarizing")}
          </p>
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
            {t("entries.transcriptionFailed")}
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
          title={t("entries.summarizeRetry")}
        >
          <AlertCircle width={13} height={13} />
          {t("entries.summarizeFailed")}
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
