import React, {
  useState,
  useCallback,
  useEffect,
  useRef,
  useMemo,
} from "react";
import { useTranslation } from "react-i18next";
import {
  ChevronUp,
  ChevronDown,
  History,
  Sparkles,
  Mic,
  RotateCcw,
  Loader2,
} from "lucide-react";
import {
  commands,
  type TranscriptionVersion,
  type HistoryEntry,
} from "@/bindings";
import { formatDateTime } from "@/utils/dateFormat";
import { toast } from "sonner";
import { listen } from "@tauri-apps/api/event";

interface VersionHistoryProps {
  entry: HistoryEntry;
}

export const VersionHistory: React.FC<VersionHistoryProps> = ({ entry }) => {
  const { t, i18n } = useTranslation();
  const [isExpanded, setIsExpanded] = useState(false);
  const [versions, setVersions] = useState<TranscriptionVersion[] | null>(null);
  const [loadingVersions, setLoadingVersions] = useState(false);
  const hasFetched = useRef(false);

  const fetchVersions = useCallback(async () => {
    setLoadingVersions(true);
    try {
      const result = await commands.getTranscriptionVersions(entry.id);
      if (result.status === "ok") {
        setVersions(result.data);
      }
    } catch (error) {
      console.error("Failed to fetch versions:", error);
    } finally {
      setLoadingVersions(false);
    }
  }, [entry.id]);

  // Fetch on first expand
  const handleToggle = useCallback(() => {
    const willExpand = !isExpanded;
    setIsExpanded(willExpand);
    if (willExpand && !hasFetched.current) {
      hasFetched.current = true;
      fetchVersions();
    }
  }, [isExpanded, fetchVersions]);

  // Refresh versions when history updates
  // If expanded, re-fetch immediately. If collapsed but previously fetched, mark stale.
  useEffect(() => {
    const setupListener = async () => {
      const unlisten = await listen("history-updated", () => {
        if (isExpanded) {
          fetchVersions();
        } else {
          // Mark as stale so next expand triggers a re-fetch
          hasFetched.current = false;
        }
      });
      return unlisten;
    };

    const unlistenPromise = setupListener();
    return () => {
      unlistenPromise.then((unlisten) => {
        if (unlisten) unlisten();
      });
    };
  }, [isExpanded, fetchVersions]);

  const versionCount = versions ? versions.length + 1 : null; // +1 for original
  const isOriginalActive = entry.post_processed_text == null;

  // Determine the single active version ID by matching text + prompt.
  // If multiple versions match (e.g., same LLM output), pick the most recent one.
  const activeVersionId = useMemo(() => {
    if (isOriginalActive || versions == null) return null;
    const matches = versions.filter(
      (v) =>
        v.text === entry.post_processed_text &&
        (v.prompt ?? "") === (entry.post_process_prompt ?? ""),
    );
    if (matches.length > 0) {
      // Return the one with the highest timestamp (most recent)
      return matches.reduce((latest, v) =>
        v.timestamp > latest.timestamp ? v : latest,
      ).id;
    }
    // Fallback: if no exact text+prompt match, try text-only match (latest)
    const textMatches = versions.filter(
      (v) => v.text === entry.post_processed_text,
    );
    if (textMatches.length > 0) {
      return textMatches.reduce((latest, v) =>
        v.timestamp > latest.timestamp ? v : latest,
      ).id;
    }
    return null;
  }, [
    versions,
    entry.post_processed_text,
    entry.post_process_prompt,
    isOriginalActive,
  ]);

  return (
    <div className="border-t border-mid-gray/20">
      {/* Toggle Bar */}
      <button
        onClick={handleToggle}
        className="w-full flex items-center gap-2 px-4 py-2 text-logo-primary hover:bg-logo-primary/5 transition-colors cursor-pointer"
      >
        <History width={14} height={14} />
        <span className="text-xs font-medium">
          {t("settings.history.versionHistory")}
        </span>
        {versionCount != null && (
          <span className="text-xs text-text/50">
            {t("settings.history.versionCount", { count: versionCount })}
          </span>
        )}
        <span className="ml-auto">
          {isExpanded ? (
            <ChevronUp width={14} height={14} />
          ) : (
            <ChevronDown width={14} height={14} />
          )}
        </span>
      </button>

      {/* Expanded Timeline */}
      {isExpanded && (
        <div className="px-4 pb-3">
          {loadingVersions && versions == null ? (
            <div className="flex items-center justify-center py-4 text-text/50">
              <Loader2 width={16} height={16} className="animate-spin" />
            </div>
          ) : versions != null ? (
            <div className="flex flex-col">
              {/* Versions in reverse chronological order (newest first) */}
              {[...versions].reverse().map((version, index) => {
                const isActive = version.id === activeVersionId;
                const isFirst = index === 0;
                return (
                  <div key={version.id}>
                    {index > 0 && <VersionConnector />}
                    <VersionCard
                      version={version}
                      entryId={entry.id}
                      isActive={isActive}
                      isLatest={isFirst}
                      language={i18n.language}
                    />
                  </div>
                );
              })}
              {/* Original transcription */}
              {versions.length > 0 && <VersionConnector />}
              <OriginalCard
                entry={entry}
                isActive={isOriginalActive}
                language={i18n.language}
              />
            </div>
          ) : null}
        </div>
      )}
    </div>
  );
};

VersionHistory.displayName = "VersionHistory";

interface ExpandableTextProps {
  text: string;
  limit: number;
  className?: string;
}

const ExpandableText: React.FC<ExpandableTextProps> = ({
  text,
  limit,
  className,
}) => {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const needsTruncation = text.length > limit;

  return (
    <span className={className}>
      {needsTruncation && !expanded ? `${text.substring(0, limit)}...` : text}
      {needsTruncation && (
        <>
          {" "}
          <button
            onClick={() => setExpanded(!expanded)}
            className="text-logo-primary hover:text-logo-primary/80 transition-colors cursor-pointer"
          >
            {expanded
              ? t("settings.history.showLess")
              : t("settings.history.showMore")}
          </button>
        </>
      )}
    </span>
  );
};

ExpandableText.displayName = "ExpandableText";

const VersionConnector: React.FC = () => (
  <div className="flex pl-4">
    <div className="w-0.5 h-4 bg-mid-gray/20" />
  </div>
);

interface VersionCardProps {
  version: TranscriptionVersion;
  entryId: number;
  isActive: boolean;
  isLatest: boolean;
  language: string;
}

const VersionCard: React.FC<VersionCardProps> = ({
  version,
  entryId,
  isActive,
  isLatest,
  language,
}) => {
  const { t } = useTranslation();

  const formattedTime = formatDateTime(String(version.timestamp), language);
  const label = isLatest ? `${formattedTime}` : formattedTime;

  return (
    <div
      className={`rounded-md border p-3 ${
        isActive
          ? "border-logo-primary/50 bg-logo-primary/10"
          : "border-mid-gray/20"
      }`}
    >
      <div className="flex items-center justify-between mb-2">
        <div className="flex items-center gap-2">
          <div
            className={`w-2 h-2 rounded-full ${
              isActive ? "bg-logo-primary" : "bg-text/30"
            }`}
          />
          <span
            className={`text-xs font-medium ${
              isActive ? "text-logo-primary" : "text-text/60"
            }`}
          >
            {label}
          </span>
        </div>
        {isActive ? (
          <span className="text-[10px] font-semibold px-2 py-0.5 rounded-full bg-logo-primary text-white">
            {t("settings.history.activeVersion")}
          </span>
        ) : (
          <RestoreButton entryId={entryId} versionId={version.id} />
        )}
      </div>
      <p
        className={`text-xs leading-relaxed mb-2 ${isActive ? "text-text/80" : "text-text/50"}`}
      >
        <ExpandableText text={version.text} limit={200} />
      </p>
      {version.prompt && (
        <div className="flex items-start gap-1">
          <Sparkles width={10} height={10} className="text-text/30 mt-0.5 shrink-0" />
          <ExpandableText
            text={version.prompt}
            limit={80}
            className="text-[11px] leading-relaxed text-text/30 whitespace-pre-wrap"
          />
        </div>
      )}
    </div>
  );
};

VersionCard.displayName = "VersionCard";

interface OriginalCardProps {
  entry: HistoryEntry;
  isActive: boolean;
  language: string;
}

const OriginalCard: React.FC<OriginalCardProps> = ({
  entry,
  isActive,
  language,
}) => {
  const { t } = useTranslation();
  const formattedTime = formatDateTime(String(entry.timestamp), language);

  return (
    <div
      className={`rounded-md border p-3 ${
        isActive
          ? "border-logo-primary/50 bg-logo-primary/10"
          : "border-mid-gray/20"
      }`}
    >
      <div className="flex items-center justify-between mb-2">
        <div className="flex items-center gap-2">
          <div
            className={`w-2 h-2 rounded-full ${
              isActive ? "bg-logo-primary" : "bg-text/30"
            }`}
          />
          <span
            className={`text-xs font-medium ${
              isActive ? "text-logo-primary" : "text-text/60"
            }`}
          >
            {formattedTime}
          </span>
        </div>
        {isActive ? (
          <span className="text-[10px] font-semibold px-2 py-0.5 rounded-full bg-logo-primary text-white">
            {t("settings.history.activeVersion")}
          </span>
        ) : (
          <RestoreButton entryId={entry.id} versionId={null} />
        )}
      </div>
      <p
        className={`text-xs leading-relaxed mb-2 ${isActive ? "text-text/80" : "text-text/50"}`}
      >
        <ExpandableText text={entry.transcription_text} limit={200} />
      </p>
      <div className="flex items-center gap-1">
        <Mic width={10} height={10} className="text-text/30" />
        <span className="text-[11px] text-text/30">
          {t("settings.history.originalVersion")}
        </span>
      </div>
    </div>
  );
};

OriginalCard.displayName = "OriginalCard";

interface RestoreButtonProps {
  entryId: number;
  versionId: number | null;
}

const RestoreButton: React.FC<RestoreButtonProps> = ({
  entryId,
  versionId,
}) => {
  const { t } = useTranslation();
  const [confirming, setConfirming] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const handleClick = async () => {
    if (!confirming) {
      setConfirming(true);
      timeoutRef.current = setTimeout(() => {
        setConfirming(false);
      }, 3000);
      return;
    }

    // Second click — do the restore
    if (timeoutRef.current) {
      clearTimeout(timeoutRef.current);
      timeoutRef.current = null;
    }
    setConfirming(false);
    setRestoring(true);

    try {
      const result = await commands.restoreVersion(entryId, versionId);
      if (result.status === "error") {
        const errorKey: Record<string, string> = {
          HISTORY_POST_PROCESS_DISABLED: "settings.history.postProcessDisabled",
          VERSION_NOT_FOUND: "settings.history.versionNotFound",
        };
        const key = errorKey[result.error] ?? "settings.history.restoreError";
        toast.error(t(key));
      }
    } catch (error) {
      toast.error(t("settings.history.restoreError"));
      console.error("Failed to restore version:", error);
    } finally {
      setRestoring(false);
    }
  };

  // Cleanup timeout on unmount
  useEffect(() => {
    return () => {
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current);
      }
    };
  }, []);

  if (restoring) {
    return (
      <span className="p-1">
        <Loader2 width={12} height={12} className="animate-spin text-text/50" />
      </span>
    );
  }

  return (
    <button
      onClick={handleClick}
      className={`flex items-center gap-1 text-[10px] font-medium px-2 py-0.5 rounded border transition-colors cursor-pointer ${
        confirming
          ? "border-logo-primary text-logo-primary"
          : "border-text/20 text-text/50 hover:text-logo-primary hover:border-logo-primary"
      }`}
    >
      {!confirming && <RotateCcw width={10} height={10} />}
      <span>
        {confirming
          ? t("settings.history.confirmRestore")
          : t("settings.history.restore")}
      </span>
    </button>
  );
};

RestoreButton.displayName = "RestoreButton";
