import React, { useCallback, useEffect, useState, useMemo } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { convertFileSrc } from "@tauri-apps/api/core";
import { readFile } from "@tauri-apps/plugin-fs";
import { createPortal } from "react-dom";
import {
  Upload,
  ArrowLeft,
  Copy,
  Check,
  Trash2,
  FileText,
  Mail,
  ChevronDown,
  ChevronUp,
  Sparkles,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import Fuse from "fuse.js";
import {
  commands,
  events,
  type HistoryEntry,
  type HistoryUpdatePayload,
} from "@/bindings";
import { useOsType } from "@/hooks/useOsType";
import { LocalFileTranscriber } from "@/components/LocalFileTranscriber";
import { AudioPlayer } from "@/components/ui/AudioPlayer";
import { formatDateTime } from "@/utils/dateFormat";
import { motion, AnimatePresence } from "framer-motion";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { FloatingBar, type ChatMessage } from "./FloatingBar";

// ---------------------------------------------------------------------------
// Meeting identification helper
// ---------------------------------------------------------------------------
const MEETING_PROMPTS = [
  "default_meeting_summary",
  "default_meeting_notes_with_actions",
] as const;

function isMeetingEntry(entry: HistoryEntry): boolean {
  return MEETING_PROMPTS.includes(
    entry.post_process_prompt as (typeof MEETING_PROMPTS)[number],
  );
}

// Custom interactive checkbox for Markdown rendering
// Custom interactive list item for task lists
const InteractiveTaskListItem: React.FC<{ children?: React.ReactNode }> = ({
  children,
}) => {
  const childrenArray = React.Children.toArray(children);

  // Find if there is an input checkbox to determine initial state
  const checkboxChild = childrenArray.find(
    (child: any) =>
      child?.type === "input" && child?.props?.type === "checkbox",
  ) as any;

  const initialChecked = !!(
    checkboxChild?.props?.checked || checkboxChild?.props?.defaultChecked
  );

  const [checked, setChecked] = useState(initialChecked);

  // Filter out the default input checkbox rendered by react-markdown / remark-gfm
  const textContent = childrenArray.filter(
    (child: any) =>
      !(child?.type === "input" && child?.props?.type === "checkbox"),
  );

  return (
    <li
      onClick={() => setChecked(!checked)}
      className="flex items-start gap-2.5 list-none my-2 select-none cursor-pointer group"
    >
      {/* Circle checkbox */}
      <div
        className={`relative flex-shrink-0 w-4 h-4 rounded-full border transition-all duration-200 mt-1 flex items-center justify-center ${
          checked
            ? "bg-forest-green border-forest-green text-charcoal"
            : "border-bark-grey/60 hover:border-forest-green bg-transparent"
        }`}
      >
        <motion.span
          initial={false}
          animate={{ scale: checked ? 1 : 0, opacity: checked ? 1 : 0 }}
          transition={{ type: "spring", stiffness: 500, damping: 30 }}
          className="flex items-center justify-center"
        >
          <Check className="w-2.5 h-2.5 stroke-[3.5] text-orange-off-white" />
        </motion.span>
      </div>

      {/* Label text with strikethrough transition */}
      <span
        className={`text-sm leading-relaxed transition-all duration-200 ${
          checked
            ? "text-bark-grey/60 line-through decoration-bark-grey/40"
            : "text-charcoal group-hover:text-obsidian"
        }`}
      >
        {textContent}
      </span>
    </li>
  );
};

// Custom blockquote to render GitHub-style alert callouts ([!NOTE], [!IMPORTANT], [!WARNING])
const AlertBlockquote: React.FC<{ children?: React.ReactNode }> = ({
  children,
}) => {
  const findText = (node: any): string => {
    if (!node) return "";
    if (typeof node === "string") return node;
    if (node.props && node.props.children) {
      if (Array.isArray(node.props.children)) {
        return node.props.children.map(findText).join("");
      }
      return findText(node.props.children);
    }
    return "";
  };

  const fullText = findText(children).trim();
  let alertType: "note" | "important" | "warning" | "none" = "none";
  let cleanChildren = children;

  if (fullText.startsWith("[!NOTE]")) {
    alertType = "note";
  } else if (fullText.startsWith("[!IMPORTANT]")) {
    alertType = "important";
  } else if (fullText.startsWith("[!WARNING]")) {
    alertType = "warning";
  }

  if (alertType !== "none") {
    const removePrefix = (node: any): any => {
      if (typeof node === "string") {
        return node.replace(/^\[!(NOTE|IMPORTANT|WARNING)\]\s*/i, "");
      }
      if (node && node.props && node.props.children) {
        return React.cloneElement(node, {
          children: Array.isArray(node.props.children)
            ? node.props.children.map(removePrefix)
            : removePrefix(node.props.children),
        });
      }
      return node;
    };

    cleanChildren = removePrefix(children);

    const borderClass =
      alertType === "important"
        ? "border-l-4 border-terracotta bg-terracotta/5"
        : alertType === "warning"
          ? "border-l-4 border-alarm-red bg-alarm-red/5"
          : "border-l-4 border-lichen-green bg-lichen-green/5";

    const titleText =
      alertType === "important"
        ? "Important"
        : alertType === "warning"
          ? "Warning"
          : "Note";

    const titleColor =
      alertType === "important"
        ? "text-terracotta font-semibold"
        : alertType === "warning"
          ? "text-alarm-red font-semibold"
          : "text-lichen-green font-semibold";

    return (
      <div className={`p-4 my-4 rounded-r-xl ${borderClass} font-sans`}>
        <div
          className={`text-xs font-bold uppercase tracking-wider mb-1 font-mono-tag ${titleColor}`}
        >
          {titleText}
        </div>
        <div className="text-sm text-bark-grey leading-relaxed select-text">
          {cleanChildren}
        </div>
      </div>
    );
  }

  return (
    <blockquote className="border-l-4 border-stone-mist pl-4 italic text-bark-grey my-4">
      {children}
    </blockquote>
  );
};

const markdownComponents = {
  li: ({ children, ...props }: any) => {
    const isTask = props.className?.includes("task-list-item");
    if (isTask) {
      return <InteractiveTaskListItem>{children}</InteractiveTaskListItem>;
    }
    return <li {...props}>{children}</li>;
  },
  blockquote: AlertBlockquote,
};

// ---------------------------------------------------------------------------
// Animation Variants
// ---------------------------------------------------------------------------
const containerVariants = {
  hidden: { opacity: 0 },
  visible: {
    opacity: 1,
    transition: { staggerChildren: 0.04 },
  },
};

const itemVariants = {
  hidden: { opacity: 0, y: 8, filter: "blur(2px)" },
  visible: {
    opacity: 1,
    y: 0,
    filter: "blur(0px)",
    transition: { type: "spring" as const, stiffness: 350, damping: 28 },
  },
};

export const MeetingsView: React.FC = () => {
  const { t, i18n } = useTranslation();
  const osType = useOsType();
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [transcriberFiles, setTranscriberFiles] = useState<string[]>([]);
  const [googleStatus, setGoogleStatus] = useState<{
    gmail_tasks_connected: boolean;
  } | null>(null);

  // Redesign state
  const [selectedMeeting, setSelectedMeeting] = useState<HistoryEntry | null>(
    null,
  );
  const [searchQuery, setSearchQuery] = useState("");
  const [detailViewMode, setDetailViewMode] = useState<
    "summary" | "transcript"
  >("summary");

  // Chat state
  const [chats, setChats] = useState<Record<number, ChatMessage[]>>({});
  const [isAsking, setIsAsking] = useState(false);

  // Transcript expand/collapse
  const [expandTranscript, setExpandTranscript] = useState(false);

  // Copy success feedback states
  const [showSummaryCopied, setShowSummaryCopied] = useState(false);
  const [showTranscriptCopied, setShowTranscriptCopied] = useState(false);

  // Google follow-up modal state
  const [showFollowUpDialog, setShowFollowUpDialog] = useState(false);
  const [followUpMeeting, setFollowUpMeeting] = useState<HistoryEntry | null>(
    null,
  );
  const [recipientsInput, setRecipientsInput] = useState("");
  const [emailsError, setEmailsError] = useState("");
  const [isSendingFollowUp, setIsSendingFollowUp] = useState(false);

  // Drag and drop state
  const [isDragActive, setIsDragActive] = useState(false);

  const handleDragEnter = (e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    if (e.dataTransfer.types.includes("Files")) {
      setIsDragActive(true);
    }
  };

  const handleDragOver = (e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
  };

  const handleDragLeave = (e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragActive(false);
  };

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragActive(false);

    if (e.dataTransfer.files && e.dataTransfer.files.length > 0) {
      const droppedFiles: string[] = [];
      const ignoredFiles: string[] = [];

      for (let i = 0; i < e.dataTransfer.files.length; i++) {
        const file = e.dataTransfer.files[i];
        const path = (file as any).path;
        if (!path) {
          ignoredFiles.push(file.name);
          continue;
        }
        const ext = path.split(".").pop()?.toLowerCase();
        const isAudio =
          ext && ["wav", "mp3", "m4a", "flac", "ogg"].includes(ext);

        if (isAudio) {
          droppedFiles.push(path);
        } else {
          ignoredFiles.push(file.name);
        }
      }

      if (ignoredFiles.length > 0) {
        toast.error(
          t("settings.meetings.ignoredUnsupportedFiles", {
            files: ignoredFiles.join(", "),
          }) || `Ignored unsupported file(s): ${ignoredFiles.join(", ")}`,
        );
      }

      if (droppedFiles.length > 0) {
        setTranscriberFiles((prev) => [
          ...prev,
          ...droppedFiles.filter((f) => !prev.includes(f)),
        ]);
      }
    }
  };

  // -------------------------------------------------------------------------
  // Data loading & events
  // -------------------------------------------------------------------------
  const loadMeetings = useCallback(async () => {
    setLoading(true);
    try {
      const [status, historyResult] = await Promise.allSettled([
        commands.getGoogleIntegrationStatus(),
        commands.getHistoryEntries(null, 100),
      ]);

      if (status.status === "fulfilled") {
        setGoogleStatus(status.value);
      }

      if (
        historyResult.status === "fulfilled" &&
        historyResult.value.status === "ok"
      ) {
        setEntries(historyResult.value.data.entries.filter(isMeetingEntry));
      }
    } catch (error) {
      console.error("Failed to load meetings:", error);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadMeetings();
  }, [loadMeetings]);

  // Live updates
  useEffect(() => {
    const unlisten = events.historyUpdatePayload.listen((event) => {
      const payload: HistoryUpdatePayload = event.payload;

      if (payload.action === "added") {
        if (isMeetingEntry(payload.entry)) {
          setEntries((prev) => [payload.entry, ...prev]);
        }
      } else if (payload.action === "updated") {
        if (isMeetingEntry(payload.entry)) {
          setEntries((prev) =>
            prev.map((e) => (e.id === payload.entry.id ? payload.entry : e)),
          );
          // Update selected meeting reference if it's the one currently open
          setSelectedMeeting((prev) => {
            if (prev && prev.id === payload.entry.id) {
              return payload.entry;
            }
            return prev;
          });
        }
      } else if (payload.action === "deleted") {
        setEntries((prev) => prev.filter((e) => e.id !== payload.id));
        setSelectedMeeting((prev) => {
          if (prev && prev.id === payload.id) {
            return null;
          }
          return prev;
        });
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  // Reset transcript expanded state when meeting changes
  useEffect(() => {
    setExpandTranscript(false);
    setDetailViewMode("summary");
  }, [selectedMeeting?.id]);

  // Helper to extract parsed title, subtitle, and time pill
  const getMeetingMetadata = useCallback(
    (entry: HistoryEntry) => {
      let displayTitle = "";
      let tags: string[] = [];

      if (entry.post_processed_text) {
        // 1. Try matching H1 title
        const h1Match = entry.post_processed_text.match(/^#\s+(.+)$/m);
        if (h1Match) {
          displayTitle = h1Match[1].trim();
        } else {
          // 2. Try JSON parsing
          try {
            const parsed = JSON.parse(entry.post_processed_text);
            displayTitle = parsed.title || parsed.summary || "";
          } catch (e) {}
        }

        // 3. Try parsing comma-separated tags line: "Tags: tag1, tag2, tag3"
        const tagsMatch = entry.post_processed_text.match(/^Tags:\s*(.+)$/im);
        if (tagsMatch) {
          tags = tagsMatch[1]
            .split(",")
            .map((t) => t.trim())
            .filter(Boolean)
            .slice(0, 3);
        }

        // 4. Fallback title generator from summary content first sentence
        if (!displayTitle) {
          let cleanText = entry.post_processed_text
            .replace(/^#+\s+.+$/gm, "") // Remove headers
            .replace(/^Tags:\s*.+$/gim, "") // Remove tags
            .trim();

          const sentenceMatch = cleanText.match(/^([^.!?\n]+)/);
          if (sentenceMatch) {
            displayTitle = sentenceMatch[1].trim();
          }
        }
      }

      if (!displayTitle) {
        if (entry.transcription_text === "") {
          displayTitle = t("settings.meetings.processing", "Processing...");
        } else {
          displayTitle = t(
            "settings.meetings.detectedMeeting",
            "Detected Meeting",
          );
        }
      }

      let monthDate = "";
      let timeStr = "";
      try {
        const date = new Date(entry.timestamp * 1000);
        monthDate = new Intl.DateTimeFormat(i18n.language, {
          month: "long",
          day: "numeric",
        }).format(date);
        timeStr = new Intl.DateTimeFormat(i18n.language, {
          hour: "numeric",
          minute: "2-digit",
          hour12: true,
        }).format(date);
      } catch (e) {
        monthDate = "Unknown Date";
        timeStr = "";
      }

      return { title: displayTitle, subtitle: monthDate, time: timeStr, tags };
    },
    [i18n.language, t],
  );

  const processedSearchEntries = useMemo(() => {
    return entries.map((entry) => {
      const { title } = getMeetingMetadata(entry);
      return {
        ...entry,
        searchTitle: title,
      };
    });
  }, [entries, getMeetingMetadata]);

  // -------------------------------------------------------------------------
  // Fuzzy Search
  // -------------------------------------------------------------------------
  const fuse = useMemo(() => {
    return new Fuse(processedSearchEntries, {
      keys: ["searchTitle", "post_processed_text"],
      threshold: 0.3,
      ignoreLocation: true,
    });
  }, [processedSearchEntries]);

  const filteredEntries = useMemo(() => {
    if (!searchQuery.trim()) return entries;
    return fuse.search(searchQuery).map((r) => r.item);
  }, [searchQuery, fuse, entries]);

  // -------------------------------------------------------------------------
  // Audio loader
  // -------------------------------------------------------------------------
  const getAudioUrl = useCallback(
    async (fileName: string): Promise<string | null> => {
      try {
        const result = await commands.getAudioFilePath(fileName);
        if (result.status !== "ok") return null;

        if (osType === "linux") {
          const fileData = await readFile(result.data);
          const blob = new Blob([fileData], { type: "audio/wav" });
          return URL.createObjectURL(blob);
        }

        return convertFileSrc(result.data, "asset");
      } catch {
        return null;
      }
    },
    [osType],
  );

  const handleLoadAudio = useCallback(
    () =>
      selectedMeeting
        ? getAudioUrl(selectedMeeting.file_name)
        : Promise.resolve(null),
    [getAudioUrl, selectedMeeting],
  );

  // -------------------------------------------------------------------------
  // Meeting Actions
  // -------------------------------------------------------------------------
  const deleteMeeting = useCallback(
    async (id: number) => {
      // Optimistic delete
      setEntries((prev) => prev.filter((e) => e.id !== id));
      if (selectedMeeting?.id === id) {
        setSelectedMeeting(null);
      }
      try {
        const result = await commands.deleteHistoryEntry(id);
        if (result.status !== "ok") {
          void loadMeetings();
        } else {
          toast.success(
            t("settings.history.deleteSuccess") || "Meeting deleted",
          );
        }
      } catch {
        void loadMeetings();
      }
    },
    [loadMeetings, selectedMeeting, t],
  );

  const handleUploadClick = async () => {
    try {
      const selected = await open({
        multiple: true,
        filters: [
          { name: "Audio", extensions: ["wav", "mp3", "m4a", "flac", "ogg"] },
        ],
      });
      if (selected) {
        setTranscriberFiles(Array.isArray(selected) ? selected : [selected]);
      }
    } catch (error) {
      console.error("Failed to open file dialog:", error);
    }
  };

  const copySummary = async (entry: HistoryEntry) => {
    const text = entry.post_processed_text || entry.transcription_text;
    try {
      await navigator.clipboard.writeText(text);
      setShowSummaryCopied(true);
      setTimeout(() => setShowSummaryCopied(false), 2000);
      toast.success(t("settings.history.copied") || "Copied summary!");
    } catch (error) {
      console.error("Failed to copy summary:", error);
    }
  };

  const copyTranscript = async (entry: HistoryEntry) => {
    try {
      await navigator.clipboard.writeText(entry.transcription_text);
      setShowTranscriptCopied(true);
      setTimeout(() => setShowTranscriptCopied(false), 2000);
      toast.success(t("settings.history.copied") || "Copied transcript!");
    } catch (error) {
      console.error("Failed to copy transcript:", error);
    }
  };

  // -------------------------------------------------------------------------
  // Follow Up Email Sending
  // -------------------------------------------------------------------------
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
    if (!followUpMeeting) return;
    const emails = validateEmails(recipientsInput);
    if (!emails) return;

    setIsSendingFollowUp(true);
    try {
      let summary = "";
      let actionItems: string[] = [];
      try {
        const parsed = JSON.parse(followUpMeeting.post_processed_text || "");
        summary = parsed.summary || "";
        actionItems = parsed.action_items || [];
      } catch (e) {
        summary =
          followUpMeeting.post_processed_text ||
          followUpMeeting.transcription_text;
        actionItems = [];
      }

      const result = await commands.sendMeetingFollowUp(
        emails,
        summary,
        actionItems,
      );

      if (result.status === "ok") {
        toast.success(
          t("settings.meetings.sendFollowUpSuccess") ||
            "Follow-up email sent successfully!",
        );
        setShowFollowUpDialog(false);
        setRecipientsInput("");
        setFollowUpMeeting(null);
      } else {
        toast.error(
          t("settings.meetings.sendFollowUpError") ||
            "Failed to send follow-up: " + result.error,
        );
      }
    } catch (error: any) {
      console.error("Failed to send meeting follow-up:", error);
      toast.error(
        t("settings.meetings.sendFollowUpError") ||
          "Failed to send follow-up email/tasks",
      );
    } finally {
      setIsSendingFollowUp(false);
    }
  };

  // -------------------------------------------------------------------------
  // Chat / AI Q&A API
  // -------------------------------------------------------------------------
  const currentChatHistory = useMemo(() => {
    if (!selectedMeeting) return [];
    return chats[selectedMeeting.id] || [];
  }, [chats, selectedMeeting]);

  const handleSendChatMessage = async (message: string) => {
    if (!selectedMeeting || isAsking || !message.trim()) return;

    // Append user message
    const userMsg: ChatMessage = { role: "user", content: message };
    setChats((prev) => ({
      ...prev,
      [selectedMeeting.id]: [...(prev[selectedMeeting.id] || []), userMsg],
    }));

    setIsAsking(true);
    try {
      const result = await commands.askMeetingQuestion(
        selectedMeeting.transcription_text,
        message,
      );
      if (result.status === "ok") {
        const assistantMsg: ChatMessage = {
          role: "assistant",
          content: result.data,
        };
        setChats((prev) => ({
          ...prev,
          [selectedMeeting.id]: [
            ...(prev[selectedMeeting.id] || []),
            assistantMsg,
          ],
        }));
      } else {
        toast.error(
          t("errors.askFailed") || "Failed to get answer: " + result.error,
        );
      }
    } catch (error) {
      console.error("Failed to ask meeting question:", error);
      toast.error(t("errors.askFailed") || "Failed to get answer");
    } finally {
      setIsAsking(false);
    }
  };

  const handleClearChat = () => {
    if (selectedMeeting) {
      setChats((prev) => ({
        ...prev,
        [selectedMeeting.id]: [],
      }));
    }
  };

  // -------------------------------------------------------------------------
  // Render details summary helper
  // -------------------------------------------------------------------------
  const getDisplaySummary = (entry: HistoryEntry) => {
    let text = "";
    if (
      entry.post_process_prompt === "default_meeting_notes_with_actions" &&
      entry.post_processed_text
    ) {
      try {
        const parsed = JSON.parse(entry.post_processed_text);
        let summary = parsed.summary || "";
        if (parsed.action_items && parsed.action_items.length > 0) {
          const actionMarkdown = parsed.action_items
            .map((item: string) => `- [ ] ${item}`)
            .join("\n");
          summary += `\n\n## Action Items\n${actionMarkdown}`;
        }
        text = summary || entry.post_processed_text;
      } catch (e) {
        text = entry.post_processed_text;
      }
    } else {
      text = entry.post_processed_text || entry.transcription_text;
    }

    if (text) {
      // Strip out the first H1 title heading
      text = text.replace(/^#\s+.+$/m, "").trim();
      // Strip out the Tags line so it's not rendered inside the markdown body
      text = text.replace(/^Tags:\s*.+$/gim, "").trim();
      // Dynamically convert ✅ bullet points to task list checkboxes for retro-compatibility
      text = text.replace(/^[•*\-\s]*✅\s*/gm, "- [ ] ");
    }
    return text;
  };

  const isGoogleConnected = !!googleStatus?.gmail_tasks_connected;

  return (
    <div
      className="w-full relative min-h-[calc(100vh-140px)]"
      onDragEnter={handleDragEnter}
      onDragOver={handleDragOver}
    >
      <AnimatePresence>
        {isDragActive && (
          <motion.div
            initial={{ opacity: 0, scale: 0.95 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.95 }}
            transition={{ duration: 0.2, ease: "easeOut" }}
            onDragOver={handleDragOver}
            onDragLeave={handleDragLeave}
            onDrop={handleDrop}
            className="absolute inset-0 z-50 flex flex-col items-center justify-center bg-[#0a0908]/90 backdrop-blur-md border-2 border-dashed border-forest-green/50 rounded-2xl m-4 transition-all duration-200"
          >
            <Upload className="w-12 h-12 text-forest-green mb-4 animate-bounce" />
            <p className="text-lg font-semibold text-charcoal">
              {t("localFileTranscriber.dropFiles")}
            </p>
            <p className="text-xs text-bark-grey mt-2">
              {t("localFileTranscriber.supportedFormats")}
            </p>
          </motion.div>
        )}
      </AnimatePresence>
      <AnimatePresence mode="wait">
        {!selectedMeeting ? (
          /* ==========================================
             1. MEETINGS LIST VIEW
             ========================================== */
          <motion.div
            key="list"
            initial={{ opacity: 0, x: -16, filter: "blur(2px)" }}
            animate={{ opacity: 1, x: 0, filter: "blur(0px)" }}
            exit={{ opacity: 0, x: -16, filter: "blur(2px)" }}
            transition={{ duration: 0.2, ease: "easeOut" }}
            className="pb-28"
          >
            {/* Header / Upload bar */}
            <div className="flex items-center justify-between mb-5">
              <h2 className="text-xs font-bold uppercase tracking-wider text-bark-grey">
                {t("settings.meetings.title")}
              </h2>
              <button
                type="button"
                onClick={handleUploadClick}
                className="flex items-center gap-1.5 text-xs font-semibold text-forest-green hover:text-deep-forest-green transition-colors bg-forest-green/10 hover:bg-forest-green/20 px-3 py-1.5 rounded-lg cursor-pointer"
              >
                <Upload className="w-3.5 h-3.5" />
                {t("settings.meetings.uploadAudio")}
              </button>
            </div>

            {loading ? (
              <div className="py-20 text-center text-bark-grey text-sm">
                <span className="w-5 h-5 border-2 border-forest-green border-t-transparent rounded-full animate-spin inline-block mr-2 align-middle"></span>
                <span>{t("settings.meetings.loading")}</span>
              </div>
            ) : filteredEntries.length === 0 ? (
              <div className="py-20 text-center text-bark-grey text-sm">
                {searchQuery
                  ? t("settings.meetings.noSearchResults")
                  : t("settings.meetings.empty")}
              </div>
            ) : (
              <motion.div
                className="bg-orange-off-white border border-stone-mist/60 rounded-2xl overflow-hidden divide-y divide-stone-mist/40 shadow-sm"
                variants={containerVariants}
                initial="hidden"
                animate="visible"
              >
                {filteredEntries.map((entry) => {
                  const {
                    title: cardTitle,
                    subtitle: cardSubtitle,
                    time: cardTime,
                  } = getMeetingMetadata(entry);
                  return (
                    <motion.div
                      key={entry.id}
                      variants={itemVariants}
                      onClick={() => setSelectedMeeting(entry)}
                      className="px-5 py-4 flex items-center justify-between hover:bg-stone-mist/35 cursor-pointer transition-colors duration-150 relative group meeting-card-collapsed"
                    >
                      <div className="space-y-1 pr-4 min-w-0 flex-1">
                        <h3 className="text-sm font-semibold text-charcoal leading-tight truncate">
                          {cardTitle}
                        </h3>
                        <p className="text-xs text-bark-grey">{cardSubtitle}</p>
                      </div>
                      <div className="flex items-center gap-3">
                        {isGoogleConnected && (
                          <button
                            type="button"
                            onClick={(e) => {
                              e.stopPropagation();
                              setFollowUpMeeting(entry);
                              setRecipientsInput("");
                              setEmailsError("");
                              setShowFollowUpDialog(true);
                            }}
                            className="flex items-center gap-1.5 text-xs font-semibold text-forest-green hover:text-deep-forest-green transition-colors bg-forest-green/10 hover:bg-forest-green/20 px-3 py-1.5 rounded-lg cursor-pointer"
                          >
                            <Mail className="w-3.5 h-3.5" />
                            <span>{t("settings.meetings.sendViaGoogle")}</span>
                          </button>
                        )}
                        {cardTime && (
                          <span className="text-xs font-medium text-bark-grey bg-stone-mist/50 px-2 py-1 rounded-full font-mono-tag">
                            {cardTime}
                          </span>
                        )}
                      </div>
                    </motion.div>
                  );
                })}
              </motion.div>
            )}
          </motion.div>
        ) : (
          /* ==========================================
             2. MEETING EXPANDED DETAILS VIEW
             ========================================== */
          <motion.div
            key="detail"
            initial={{ opacity: 0, x: 16, filter: "blur(2px)" }}
            animate={{ opacity: 1, x: 0, filter: "blur(0px)" }}
            exit={{ opacity: 0, x: 16, filter: "blur(2px)" }}
            transition={{ duration: 0.2, ease: "easeOut" }}
            className="pb-28 max-w-4xl mx-auto"
          >
            {/* Top Navigation Row */}
            <div className="flex items-center justify-between mb-6">
              <button
                type="button"
                onClick={() => setSelectedMeeting(null)}
                className="flex items-center gap-1.5 text-xs font-semibold text-bark-grey hover:text-charcoal transition-colors cursor-pointer"
              >
                <ArrowLeft className="w-4 h-4" />
                <span>{t("settings.meetings.backToMeetings")}</span>
              </button>

              {/* Detail Action buttons */}
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => copySummary(selectedMeeting)}
                  className="p-2 rounded-xl text-bark-grey hover:text-charcoal hover:bg-orange-off-white border border-stone-mist/40 transition-colors flex items-center justify-center cursor-pointer shadow-sm"
                  title={t("settings.history.copyToClipboard")}
                >
                  {showSummaryCopied ? (
                    <Check className="w-4 h-4 text-forest-green" />
                  ) : (
                    <Copy className="w-4 h-4" />
                  )}
                </button>
                {isGoogleConnected && (
                  <button
                    type="button"
                    onClick={() => {
                      setFollowUpMeeting(selectedMeeting);
                      setRecipientsInput("");
                      setEmailsError("");
                      setShowFollowUpDialog(true);
                    }}
                    className="p-2 rounded-xl text-bark-grey hover:text-charcoal hover:bg-orange-off-white border border-stone-mist/40 transition-colors flex items-center justify-center cursor-pointer shadow-sm"
                    title={t("settings.meetings.sendViaGoogle")}
                  >
                    <Mail className="w-4 h-4" />
                  </button>
                )}
                <button
                  type="button"
                  onClick={() => {
                    if (
                      confirm(
                        t("settings.meetings.deleteConfirmation") ||
                          "Are you sure you want to delete this meeting summary?",
                      )
                    ) {
                      void deleteMeeting(selectedMeeting.id);
                    }
                  }}
                  className="p-2 rounded-xl text-bark-grey hover:text-alarm-red hover:bg-alarm-red/10 border border-stone-mist/40 transition-colors flex items-center justify-center cursor-pointer shadow-sm"
                  title={t("settings.history.delete")}
                >
                  <Trash2 className="w-4 h-4" />
                </button>
              </div>
            </div>

            {/* Title & Metadata & Tags */}
            <div className="mb-6 space-y-3">
              <div className="flex flex-wrap items-center gap-3">
                <h2 className="text-2xl font-bold text-charcoal font-cooper leading-snug">
                  {getMeetingMetadata(selectedMeeting).title}
                </h2>
                {getMeetingMetadata(selectedMeeting).tags.map((tag) => (
                  <span
                    key={tag}
                    className="text-[10px] font-semibold text-tide-teal bg-tide-teal/10 border border-tide-teal/20 px-2.5 py-0.5 rounded-full font-mono uppercase tracking-wider"
                  >
                    {tag}
                  </span>
                ))}
              </div>
              <p className="text-sm text-bark-grey font-medium">
                {formatDateTime(
                  String(selectedMeeting.timestamp),
                  i18n.language,
                )}
              </p>
            </div>

            {/* Floating Vertical Toggle Menu on the right side */}
            {createPortal(
              <div className="fixed right-6 lg:right-auto lg:left-[calc(50%+472px)] top-1/2 -translate-y-1/2 flex flex-col items-center gap-2 p-1.5 bg-orange-off-white/90 border border-stone-mist/80 rounded-full shadow-lg backdrop-blur-md z-40">
                <button
                  type="button"
                  onClick={() => setDetailViewMode("summary")}
                  className={`p-2.5 rounded-full transition-colors cursor-pointer ${
                    detailViewMode === "summary"
                      ? "bg-forest-green text-orange-off-white"
                      : "text-bark-grey hover:text-charcoal hover:bg-stone-mist/40"
                  }`}
                  title={t("settings.meetings.summaryTab") || "Summary"}
                  aria-label={t("settings.meetings.summaryTab") || "Summary"}
                >
                  <Sparkles className="w-4 h-4" />
                </button>
                <button
                  type="button"
                  onClick={() => setDetailViewMode("transcript")}
                  className={`p-2.5 rounded-full transition-colors cursor-pointer ${
                    detailViewMode === "transcript"
                      ? "bg-forest-green text-orange-off-white"
                      : "text-bark-grey hover:text-charcoal hover:bg-stone-mist/40"
                  }`}
                  title={t("settings.meetings.transcriptTab") || "Transcript"}
                  aria-label={
                    t("settings.meetings.transcriptTab") || "Transcript"
                  }
                >
                  <FileText className="w-4 h-4" />
                </button>
              </div>,
              document.body,
            )}

            {/* Toggle-based Details Rendering */}
            {detailViewMode === "summary" ? (
              /* Summary Section (Direct area, no card) */
              <div className="text-sm leading-relaxed text-charcoal select-text markdown-summary min-h-[200px]">
                {selectedMeeting.post_processed_text ? (
                  <ReactMarkdown
                    remarkPlugins={[remarkGfm]}
                    components={markdownComponents}
                  >
                    {getDisplaySummary(selectedMeeting)}
                  </ReactMarkdown>
                ) : selectedMeeting.transcription_text === "" ? (
                  <div className="flex items-center gap-2 text-bark-grey py-1">
                    <span className="w-4 h-4 border-2 border-forest-green border-t-transparent rounded-full animate-spin"></span>
                    <span>{t("settings.meetings.processing")}</span>
                  </div>
                ) : (
                  <p className="text-pebble italic">
                    {t("settings.meetings.summaryFailed")}
                  </p>
                )}
              </div>
            ) : (
              /* Audio + Transcript Section */
              <div className="space-y-6">
                <div>
                  <AudioPlayer
                    onLoadRequest={handleLoadAudio}
                    className="w-full"
                  />
                </div>
                <div className="border border-stone-mist/40 rounded-2xl bg-orange-off-white/40 overflow-hidden shadow-sm">
                  <div className="px-5 py-4 space-y-3">
                    <div className="flex items-center justify-between border-b border-stone-mist/20 pb-2">
                      <span className="text-xs font-bold uppercase tracking-wider text-bark-grey">
                        {t("settings.meetings.fullTranscript")}
                      </span>
                      <button
                        type="button"
                        onClick={() => copyTranscript(selectedMeeting)}
                        className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-semibold text-bark-grey hover:text-charcoal border border-stone-mist/40 bg-orange-off-white rounded-lg transition-colors cursor-pointer"
                      >
                        {showTranscriptCopied ? (
                          <Check className="w-3.5 h-3.5 text-forest-green" />
                        ) : (
                          <Copy className="w-3.5 h-3.5" />
                        )}
                        <span>
                          {showTranscriptCopied
                            ? t("settings.meetings.copied") || "Copied"
                            : t("settings.meetings.copy") || "Copy"}
                        </span>
                      </button>
                    </div>
                    <div className="text-sm text-bark-grey whitespace-pre-wrap leading-relaxed select-text font-normal font-sans max-h-96 overflow-y-auto pr-2 scrollbar-thin">
                      {selectedMeeting.transcription_text || (
                        <p className="italic text-pebble">
                          {t(
                            "settings.meetings.noTranscript",
                            "No transcript text available.",
                          )}
                        </p>
                      )}
                    </div>
                  </div>
                </div>
              </div>
            )}
          </motion.div>
        )}
      </AnimatePresence>

      {/* ==========================================
         3. DUAL-PURPOSE FLOATING BAR
         ========================================== */}
      {!loading &&
        createPortal(
          <FloatingBar
            mode={selectedMeeting ? "chat" : "search"}
            searchQuery={searchQuery}
            onSearchChange={setSearchQuery}
            chatHistory={currentChatHistory}
            onSendChatMessage={handleSendChatMessage}
            isSendingChat={isAsking}
            onClearChat={handleClearChat}
          />,
          document.body,
        )}

      {/* ==========================================
         4. FOLLOW UP DIALOG
         ========================================== */}
      {showFollowUpDialog && followUpMeeting && (
        <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50 p-4">
          <div className="bg-orange-off-white border border-stone-mist rounded-2xl max-w-md w-full p-6 space-y-4 shadow-2xl backdrop-blur-sm">
            <h3 className="text-base font-semibold text-charcoal font-cooper">
              {t("settings.meetings.sendFollowUpTitle")}
            </h3>

            <div className="space-y-1.5">
              <label className="text-xs font-semibold uppercase tracking-wider text-bark-grey">
                {t("settings.meetings.recipientsLabel")}
              </label>
              <textarea
                value={recipientsInput}
                onChange={(e) => {
                  setRecipientsInput(e.target.value);
                  setEmailsError("");
                }}
                placeholder={t("settings.meetings.recipientsPlaceholder")}
                className="w-full h-24 px-3 py-2 bg-warm-bone/45 border border-stone-mist/60 rounded-xl text-sm text-charcoal placeholder-pebble focus:outline-none focus:border-forest-green"
              />
              {emailsError && (
                <p className="text-xs text-alarm-red">{emailsError}</p>
              )}
            </div>

            <div className="flex justify-end gap-2 pt-2">
              <button
                type="button"
                onClick={() => {
                  setShowFollowUpDialog(false);
                  setRecipientsInput("");
                  setEmailsError("");
                  setFollowUpMeeting(null);
                }}
                className="px-4 py-2 text-xs font-semibold text-bark-grey hover:text-charcoal bg-warm-bone/60 border border-stone-mist/40 rounded-xl transition-colors cursor-pointer"
              >
                {t("settings.meetings.cancel")}
              </button>
              <button
                type="button"
                onClick={handleSendFollowUp}
                disabled={isSendingFollowUp}
                className="px-4 py-2 text-xs font-semibold bg-forest-green hover:bg-deep-forest-green text-orange-off-white disabled:opacity-50 rounded-xl transition-colors cursor-pointer flex items-center gap-1.5"
              >
                {isSendingFollowUp && (
                  <span className="w-3.5 h-3.5 border-2 border-orange-off-white border-t-transparent rounded-full animate-spin"></span>
                )}
                <span>
                  {isSendingFollowUp
                    ? t("settings.meetings.sending")
                    : t("settings.meetings.send")}
                </span>
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Local file transcriber Modal */}
      <AnimatePresence>
        {transcriberFiles.length > 0 && (
          <LocalFileTranscriber
            initialFiles={transcriberFiles}
            onClose={() => setTranscriberFiles([])}
            onSuccess={() => {
              void loadMeetings();
            }}
          />
        )}
      </AnimatePresence>
    </div>
  );
};
