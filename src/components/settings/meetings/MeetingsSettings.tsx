import React, { useCallback, useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { readFile } from "@tauri-apps/plugin-fs";
import {
  Check,
  Copy,
  Trash2,
  ChevronDown,
  ChevronUp,
  FileText,
  Upload,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { open } from "@tauri-apps/plugin-dialog";
import {
  commands,
  events,
  type HistoryEntry,
  type HistoryUpdatePayload,
} from "@/bindings";
import { useOsType } from "@/hooks/useOsType";
import { formatDateTime } from "@/utils/dateFormat";
import { AudioPlayer } from "../../ui/AudioPlayer";
import { LocalFileTranscriber } from "../../LocalFileTranscriber";

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

export const MeetingsSettings: React.FC = () => {
  const { t } = useTranslation();
  const osType = useOsType();
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [transcriberFiles, setTranscriberFiles] = useState<string[]>([]);

  const handleUploadClick = async () => {
    try {
      const selected = await open({
        multiple: true,
        filters: [
          {
            name: "Audio",
            extensions: ["wav", "mp3", "m4a", "flac", "ogg"],
          },
        ],
      });
      if (selected) {
        const newFiles = Array.isArray(selected) ? selected : [selected];
        setTranscriberFiles(newFiles);
      }
    } catch (error) {
      console.error("Failed to open file dialog:", error);
    }
  };

  const loadMeetings = useCallback(async () => {
    setLoading(true);
    try {
      // Fetch a larger page size to ensure we grab recent meetings
      const result = await commands.getHistoryEntries(null, 100);
      if (result.status === "ok") {
        const meetingEntries = result.data.entries.filter(
          (e) => e.post_process_prompt === "default_meeting_summary",
        );
        setEntries(meetingEntries);
      }
    } catch (error) {
      console.error("Failed to load meeting entries:", error);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadMeetings();
  }, [loadMeetings]);

  // Listen for new meeting entries added or updated
  useEffect(() => {
    const unlisten = events.historyUpdatePayload.listen((event) => {
      const payload: HistoryUpdatePayload = event.payload;
      if (payload.action === "added") {
        if (payload.entry.post_process_prompt === "default_meeting_summary") {
          setEntries((prev) => [payload.entry, ...prev]);
        }
      } else if (payload.action === "updated") {
        if (payload.entry.post_process_prompt === "default_meeting_summary") {
          setEntries((prev) =>
            prev.map((e) => (e.id === payload.entry.id ? payload.entry : e)),
          );
        }
      } else if (payload.action === "deleted") {
        setEntries((prev) => prev.filter((e) => e.id !== payload.id));
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

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

  const deleteMeetingEntry = async (id: number) => {
    setEntries((prev) => prev.filter((e) => e.id !== id));
    try {
      const result = await commands.deleteHistoryEntry(id);
      if (result.status !== "ok") {
        loadMeetings();
      }
    } catch (error) {
      console.error("Failed to delete meeting entry:", error);
      loadMeetings();
    }
  };

  let content: React.ReactNode;

  if (loading) {
    content = (
      <div className="px-4 py-8 text-center text-text/60">
        {t("settings.meetings.loading")}
      </div>
    );
  } else if (entries.length === 0) {
    content = (
      <div className="px-4 py-8 text-center text-text/60">
        {t("settings.meetings.empty")}
      </div>
    );
  } else {
    content = (
      <div className="divide-y divide-mid-gray/20">
        {entries.map((entry) => (
          <MeetingEntryComponent
            key={entry.id}
            entry={entry}
            getAudioUrl={getAudioUrl}
            deleteMeeting={deleteMeetingEntry}
          />
        ))}
      </div>
    );
  }

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <div className="space-y-2">
        <div className="px-4 flex items-center justify-between">
          <h2 className="text-xs font-medium text-mid-gray uppercase tracking-wide">
            {t("settings.meetings.title")}
          </h2>
          <button
            onClick={handleUploadClick}
            className="flex items-center gap-1.5 text-xs font-medium text-logo-primary hover:text-logo-primary/80 transition-colors bg-logo-primary/10 px-2 py-1 rounded-md"
          >
            <Upload className="w-3.5 h-3.5" />
            Upload Audio
          </button>
        </div>
        <div className="bg-background border border-mid-gray/20 rounded-lg overflow-visible">
          {content}
        </div>
      </div>
      
      {transcriberFiles.length > 0 && (
        <LocalFileTranscriber
          initialFiles={transcriberFiles}
          onClose={() => setTranscriberFiles([])}
          onSuccess={() => {
            loadMeetings();
          }}
        />
      )}
    </div>
  );
};

interface MeetingEntryProps {
  entry: HistoryEntry;
  getAudioUrl: (fileName: string) => Promise<string | null>;
  deleteMeeting: (id: number) => Promise<void>;
}

const MeetingEntryComponent: React.FC<MeetingEntryProps> = ({
  entry,
  getAudioUrl,
  deleteMeeting,
}) => {
  const { t, i18n } = useTranslation();
  const [showSummaryCopied, setShowSummaryCopied] = useState(false);
  const [showTranscriptCopied, setShowTranscriptCopied] = useState(false);
  const [expandTranscript, setExpandTranscript] = useState(false);

  const handleLoadAudio = useCallback(
    () => getAudioUrl(entry.file_name),
    [getAudioUrl, entry.file_name],
  );

  const copySummary = async () => {
    const text = entry.post_processed_text || entry.transcription_text;
    try {
      await navigator.clipboard.writeText(text);
      setShowSummaryCopied(true);
      setTimeout(() => setShowSummaryCopied(false), 2000);
    } catch (error) {
      console.error("Failed to copy summary:", error);
    }
  };

  const copyTranscript = async () => {
    try {
      await navigator.clipboard.writeText(entry.transcription_text);
      setShowTranscriptCopied(true);
      setTimeout(() => setShowTranscriptCopied(false), 2000);
    } catch (error) {
      console.error("Failed to copy transcript:", error);
    }
  };

  const handleDelete = async () => {
    try {
      await deleteMeeting(entry.id);
    } catch (error) {
      console.error("Failed to delete meeting:", error);
      toast.error(t("settings.history.deleteError"));
    }
  };

  const formattedDate = formatDateTime(String(entry.timestamp), i18n.language);

  return (
    <div className="px-4 py-4 flex flex-col gap-4">
      <div className="flex justify-between items-center border-b border-mid-gray/10 pb-2">
        <div>
          <p className="text-sm font-semibold text-text">{formattedDate}</p>
        </div>
        <div className="flex items-center gap-1">
          <IconButton
            onClick={handleDelete}
            title={t("settings.history.delete")}
          >
            <Trash2 width={16} height={16} />
          </IconButton>
        </div>
      </div>

      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <h4 className="text-xs font-semibold uppercase tracking-wider text-mid-gray">
            {t("settings.meetings.summary")}
          </h4>
          <IconButton
            onClick={copySummary}
            title={t("settings.history.copyToClipboard")}
          >
            {showSummaryCopied ? (
              <Check width={14} height={14} />
            ) : (
              <Copy width={14} height={14} />
            )}
          </IconButton>
        </div>
        <div className="p-3 bg-mid-gray/5 rounded-md border border-mid-gray/10 text-sm text-text/90 whitespace-pre-wrap select-text">
          {entry.post_processed_text || t("settings.meetings.summaryFailed")}
        </div>
      </div>

      <div className="space-y-2">
        <button
          onClick={() => setExpandTranscript(!expandTranscript)}
          className="flex items-center justify-between w-full text-left cursor-pointer hover:bg-mid-gray/5 p-1 rounded transition-colors"
        >
          <div className="flex items-center gap-2">
            <FileText className="w-4 h-4 text-mid-gray" />
            <span className="text-xs font-semibold uppercase tracking-wider text-mid-gray">
              {t("settings.meetings.fullTranscript")}
            </span>
          </div>
          {expandTranscript ? (
            <ChevronUp className="w-4 h-4 text-mid-gray" />
          ) : (
            <ChevronDown className="w-4 h-4 text-mid-gray" />
          )}
        </button>

        {expandTranscript && (
          <div className="space-y-2 pt-1">
            <div className="flex justify-end">
              <IconButton
                onClick={copyTranscript}
                title={t("settings.history.copyToClipboard")}
              >
                {showTranscriptCopied ? (
                  <Check width={14} height={14} />
                ) : (
                  <Copy width={14} height={14} />
                )}
              </IconButton>
            </div>
            <div className="p-3 bg-mid-gray/5 rounded-md border border-mid-gray/10 text-sm text-text/80 whitespace-pre-wrap select-text">
              {entry.transcription_text}
            </div>
          </div>
        )}
      </div>

      <AudioPlayer onLoadRequest={handleLoadAudio} className="w-full mt-1" />
    </div>
  );
};
