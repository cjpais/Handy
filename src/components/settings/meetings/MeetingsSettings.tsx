import React, { useCallback, useEffect, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { readFile } from "@tauri-apps/plugin-fs";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  Check,
  Copy,
  Trash2,
  ChevronDown,
  ChevronUp,
  FileText,
  Upload,
  Mail,
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
  const [isGoogleConnected, setIsGoogleConnected] = useState(false);
  const [isConnecting, setIsConnecting] = useState(false);

  const checkGoogleAuth = useCallback(async () => {
    try {
      const result = (await invoke("get_google_auth_status")) as any;
      if (result && typeof result === "object" && "status" in result) {
        if (result.status === "ok") {
          setIsGoogleConnected(!!result.data);
        }
      } else {
        setIsGoogleConnected(!!result);
      }
    } catch (error) {
      console.error("Failed to check Google auth status:", error);
    }
  }, []);

  const handleConnectGoogle = async () => {
    setIsConnecting(true);
    try {
      const result = (await invoke("start_google_oauth")) as any;
      let success = false;
      if (result && typeof result === "object" && "status" in result) {
        if (result.status === "ok") {
          success = result.data === "success";
        } else {
          toast.error(
            t("settings.meetings.googleConnectError") ||
              "OAuth connection failed: " + result.error,
          );
        }
      } else {
        success = result === "success";
      }
      if (success) {
        toast.success(
          t("settings.meetings.googleConnectSuccess") ||
            "Google Services connected successfully!",
        );
        setIsGoogleConnected(true);
      } else {
        toast.error(
          t("settings.meetings.googleConnectError") ||
            "Failed to connect Google Services.",
        );
      }
    } catch (error: any) {
      console.error("Failed to connect Google services:", error);
      toast.error(
        t("settings.meetings.googleConnectError") ||
          "Failed to start Google OAuth flow",
      );
    } finally {
      setIsConnecting(false);
    }
  };

  const handleDisconnectGoogle = async () => {
    try {
      const result = (await invoke("disconnect_google_auth")) as any;
      let success = true;
      if (
        result &&
        typeof result === "object" &&
        "status" in result &&
        result.status !== "ok"
      ) {
        success = false;
        toast.error(
          t("settings.meetings.googleDisconnectError") ||
            "Failed to disconnect: " + result.error,
        );
      }
      if (success) {
        toast.success(
          t("settings.meetings.googleDisconnectSuccess") ||
            "Google Services disconnected",
        );
        setIsGoogleConnected(false);
      }
    } catch (error) {
      console.error("Failed to disconnect Google services:", error);
      toast.error(
        t("settings.meetings.googleDisconnectError") ||
          "Failed to disconnect Google Services",
      );
    }
  };

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
      await checkGoogleAuth();
      // Fetch a larger page size to ensure we grab recent meetings
      const result = await commands.getHistoryEntries(null, 100);
      if (result.status === "ok") {
        const meetingEntries = result.data.entries.filter(
          (e) =>
            e.post_process_prompt === "default_meeting_summary" ||
            e.post_process_prompt === "default_meeting_notes_with_actions",
        );
        setEntries(meetingEntries);
      }
    } catch (error) {
      console.error("Failed to load meeting entries:", error);
    } finally {
      setLoading(false);
    }
  }, [checkGoogleAuth]);

  useEffect(() => {
    loadMeetings();
  }, [loadMeetings]);

  // Listen for new meeting entries added or updated
  useEffect(() => {
    const unlisten = events.historyUpdatePayload.listen((event) => {
      const payload: HistoryUpdatePayload = event.payload;
      if (payload.action === "added") {
        if (
          payload.entry.post_process_prompt === "default_meeting_summary" ||
          payload.entry.post_process_prompt ===
            "default_meeting_notes_with_actions"
        ) {
          setEntries((prev) => [payload.entry, ...prev]);
        }
      } else if (payload.action === "updated") {
        if (
          payload.entry.post_process_prompt === "default_meeting_summary" ||
          payload.entry.post_process_prompt ===
            "default_meeting_notes_with_actions"
        ) {
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
            isGoogleConnected={isGoogleConnected}
          />
        ))}
      </div>
    );
  }

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      {/* Google Services Connection Section */}
      <div className="bg-background border border-mid-gray/20 rounded-lg p-4 flex items-center justify-between">
        <div className="space-y-1">
          <h3 className="text-sm font-semibold text-text">
            {t("settings.meetings.googleServices")}
          </h3>
          <p className="text-xs text-mid-gray">
            {isGoogleConnected
              ? t("settings.meetings.googleConnected")
              : t("settings.meetings.googleDisconnected")}
          </p>
        </div>
        <div>
          {isGoogleConnected ? (
            <button
              onClick={handleDisconnectGoogle}
              className="px-3 py-1.5 text-xs font-medium bg-red-600/10 text-red-500 hover:bg-red-600/20 rounded-md transition-colors cursor-pointer google-disconnect-btn"
            >
              {t("settings.meetings.googleDisconnect")}
            </button>
          ) : (
            <button
              onClick={handleConnectGoogle}
              disabled={isConnecting}
              className="px-3 py-1.5 text-xs font-medium bg-logo-primary text-white hover:bg-logo-primary/95 disabled:opacity-55 rounded-md transition-colors cursor-pointer google-connect-btn"
            >
              {isConnecting
                ? t("settings.meetings.googleConnecting")
                : t("settings.meetings.googleConnect")}
            </button>
          )}
        </div>
      </div>

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
            {t("settings.meetings.uploadAudio")}
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
  isGoogleConnected: boolean;
}

const MeetingEntryComponent: React.FC<MeetingEntryProps> = ({
  entry,
  getAudioUrl,
  deleteMeeting,
  isGoogleConnected,
}) => {
  const { t, i18n } = useTranslation();
  const [showSummaryCopied, setShowSummaryCopied] = useState(false);
  const [showTranscriptCopied, setShowTranscriptCopied] = useState(false);
  const [expandTranscript, setExpandTranscript] = useState(false);

  const [showFollowUpDialog, setShowFollowUpDialog] = useState(false);
  const [recipientsInput, setRecipientsInput] = useState("");
  const [emailsError, setEmailsError] = useState("");
  const [isSending, setIsSending] = useState(false);

  const validateEmails = (input: string): string[] | null => {
    const trimmed = input.trim();
    if (!trimmed) {
      setEmailsError(
        t("settings.meetings.recipientsRequired") ||
          "Recipient email is required.",
      );
      return null;
    }
    const emails = trimmed.split(/[\s,]+/).filter(Boolean);
    const emailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
    for (const email of emails) {
      if (!emailRegex.test(email)) {
        setEmailsError(
          t("settings.meetings.invalidEmail", { email }) ||
            `Invalid email address: ${email}`,
        );
        return null;
      }
    }
    setEmailsError("");
    return emails;
  };

  const handleSendFollowUp = async () => {
    const emails = validateEmails(recipientsInput);
    if (!emails) {
      return;
    }

    setIsSending(true);
    try {
      let summary = "";
      let actionItems: string[] = [];
      try {
        const parsed = JSON.parse(entry.post_processed_text || "");
        summary = parsed.summary || "";
        actionItems = parsed.action_items || [];
      } catch (e) {
        summary = entry.post_processed_text || entry.transcription_text;
        actionItems = [];
      }

      const result = (await invoke("send_meeting_follow_up", {
        recipients: emails,
        summary,
        action_items: actionItems,
        actionItems,
      })) as any;

      let success = true;
      if (result && typeof result === "object" && "status" in result) {
        if (result.status !== "ok") {
          success = false;
          toast.error(
            t("settings.meetings.sendFollowUpError") ||
              "Failed to send follow-up: " + result.error,
          );
        }
      }

      if (success) {
        toast.success(
          t("settings.meetings.sendFollowUpSuccess") ||
            "Follow-up email and tasks sent successfully!",
        );
        setShowFollowUpDialog(false);
        setRecipientsInput("");
      }
    } catch (error: any) {
      console.error("Failed to send meeting follow-up:", error);
      toast.error(
        t("settings.meetings.sendFollowUpError") ||
          "Failed to send follow-up email/tasks",
      );
    } finally {
      setIsSending(false);
    }
  };

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

  const displaySummary = React.useMemo(() => {
    if (
      entry.post_process_prompt === "default_meeting_notes_with_actions" &&
      entry.post_processed_text
    ) {
      try {
        const parsed = JSON.parse(entry.post_processed_text);
        return parsed.summary || entry.post_processed_text;
      } catch (e) {
        return entry.post_processed_text;
      }
    }
    return entry.post_processed_text || entry.transcription_text;
  }, [
    entry.post_process_prompt,
    entry.post_processed_text,
    entry.transcription_text,
  ]);

  return (
    <div className="px-4 py-4 flex flex-col gap-4">
      <div className="flex justify-between items-center border-b border-mid-gray/10 pb-2">
        <div>
          <p className="text-sm font-semibold text-text">{formattedDate}</p>
        </div>
        <div className="flex items-center gap-2">
          {isGoogleConnected && (
            <button
              onClick={() => setShowFollowUpDialog(true)}
              className="flex items-center gap-1.5 text-xs font-medium text-logo-primary hover:text-logo-primary/80 transition-colors bg-logo-primary/10 px-2 py-1 rounded-md cursor-pointer send-via-google-btn"
              title={t("settings.meetings.sendViaGoogle")}
            >
              <Mail width={14} height={14} />
              {t("settings.meetings.sendViaGoogle")}
            </button>
          )}
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
        <div className="p-3 bg-mid-gray/5 rounded-md border border-mid-gray/10 text-sm text-text/90 select-text markdown-summary">
          {entry.post_processed_text ? (
            <ReactMarkdown remarkPlugins={[remarkGfm]}>
              {entry.post_processed_text}
            </ReactMarkdown>
          ) : entry.transcription_text === "" ? (
            <div className="flex items-center gap-2 text-mid-gray py-1">
              <span className="w-3.5 h-3.5 border-2 border-logo-primary border-t-transparent rounded-full animate-spin"></span>
              <span>{t("settings.meetings.processing")}</span>
            </div>
          ) : (
            t("settings.meetings.summaryFailed")
          )}
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

      {showFollowUpDialog && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4 follow-up-dialog">
          <div className="bg-background border border-mid-gray/20 rounded-lg max-w-md w-full p-6 space-y-4 shadow-xl">
            <h3 className="text-base font-semibold text-text">
              {t("settings.meetings.sendFollowUpTitle")}
            </h3>

            <div className="space-y-1.5">
              <label className="text-xs font-semibold uppercase tracking-wider text-mid-gray">
                {t("settings.meetings.recipientsLabel")}
              </label>
              <textarea
                value={recipientsInput}
                onChange={(e) => {
                  setRecipientsInput(e.target.value);
                  setEmailsError("");
                }}
                disabled={isSending}
                placeholder={t("settings.meetings.recipientsPlaceholder")}
                className="w-full h-20 px-3 py-2 bg-mid-gray/5 border border-mid-gray/20 rounded-md text-sm text-text focus:outline-none focus:border-logo-primary resize-none recipients-input"
              />
              {emailsError && (
                <p className="text-xs font-medium text-red-500 error-message">
                  {emailsError}
                </p>
              )}
            </div>

            <div className="flex justify-end gap-2 pt-2 border-t border-mid-gray/10">
              <button
                onClick={() => {
                  setShowFollowUpDialog(false);
                  setRecipientsInput("");
                  setEmailsError("");
                }}
                disabled={isSending}
                className="px-4 py-2 text-xs font-medium text-text hover:bg-mid-gray/10 rounded-md transition-colors cursor-pointer cancel-btn"
              >
                {t("settings.meetings.cancel")}
              </button>
              <button
                onClick={handleSendFollowUp}
                disabled={isSending}
                className="px-4 py-2 text-xs font-medium bg-logo-primary text-white hover:bg-logo-primary/95 disabled:opacity-55 rounded-md transition-colors cursor-pointer flex items-center gap-1.5 send-btn"
              >
                {isSending ? (
                  <>
                    <span className="w-3.5 h-3.5 border-2 border-white border-t-transparent rounded-full animate-spin"></span>
                    {t("settings.meetings.sending")}
                  </>
                ) : (
                  t("settings.meetings.send")
                )}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
