import React, { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { SettingContainer } from "../../ui/SettingContainer";
import { Button } from "../../ui/Button";

// Maximum number of lines kept in memory / rendered at once.
const MAX_LINES = 1000;
// Incoming logs are buffered and flushed on this cadence so a burst of log
// activity can never trigger a render per line.
const FLUSH_INTERVAL_MS = 250;

// Payload emitted by tauri-plugin-log's `Webview` target on the `log://log`
// event. `level` is the numeric LogLevel repr: Trace=1, Debug=2, Info=3,
// Warn=4, Error=5. `message` is the raw log message (no timestamp/target).
interface LogEventPayload {
  message: string;
  level: number;
}

interface LogLine {
  id: number;
  level: number;
  time: string;
  message: string;
}

// Level accents carry a light-theme color plus a brighter `dark:` variant, so
// they read on both the light code-block surface and the dark console surface.
const LEVEL_META: Record<
  number,
  { tag: string; tagClass: string; msgClass: string }
> = {
  1: {
    tag: "TRACE",
    tagClass: "text-mid-gray",
    msgClass: "text-mid-gray",
  },
  2: {
    tag: "DEBUG",
    tagClass: "text-sky-600 dark:text-sky-400",
    msgClass: "text-text/80",
  },
  3: {
    tag: "INFO",
    tagClass: "text-emerald-600 dark:text-emerald-400",
    msgClass: "text-text",
  },
  4: {
    tag: "WARN",
    tagClass: "text-amber-600 dark:text-amber-400",
    msgClass: "text-amber-700 dark:text-amber-300",
  },
  5: {
    tag: "ERROR",
    tagClass: "text-red-600 dark:text-red-400",
    msgClass: "text-red-700 dark:text-red-300",
  },
};

const UNKNOWN_META = {
  tag: "LOG",
  tagClass: "text-mid-gray",
  msgClass: "text-text",
};

const metaFor = (level: number) => LEVEL_META[level] ?? UNKNOWN_META;

const formatTime = (date: Date): string => {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(
    date.getSeconds(),
  )}`;
};

interface LiveLogViewerProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const LiveLogViewer: React.FC<LiveLogViewerProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const [logs, setLogs] = useState<LogLine[]>([]);
  const [paused, setPaused] = useState(false);
  const [copied, setCopied] = useState(false);

  const pendingRef = useRef<LogLine[]>([]);
  const idRef = useRef(0);
  const pausedRef = useRef(false);
  const pinnedRef = useRef(true);
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    pausedRef.current = paused;
  }, [paused]);

  // Subscribe to the backend log stream. Lines land in a ref buffer rather than
  // state so high log volume never overwhelms React.
  useEffect(() => {
    const unlisten = listen<LogEventPayload>("log://log", (event) => {
      const line: LogLine = {
        id: idRef.current++,
        level: event.payload.level,
        time: formatTime(new Date()),
        message: event.payload.message,
      };
      const pending = pendingRef.current;
      pending.push(line);
      if (pending.length > MAX_LINES) {
        pending.splice(0, pending.length - MAX_LINES);
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  // Flush buffered lines into state on a fixed cadence to cap re-renders.
  useEffect(() => {
    const interval = setInterval(() => {
      if (pausedRef.current || pendingRef.current.length === 0) return;
      const incoming = pendingRef.current;
      pendingRef.current = [];
      setLogs((prev) => {
        const next = prev.concat(incoming);
        return next.length > MAX_LINES
          ? next.slice(next.length - MAX_LINES)
          : next;
      });
    }, FLUSH_INTERVAL_MS);

    return () => clearInterval(interval);
  }, []);

  // Keep the view pinned to the latest line unless the user has scrolled up.
  useEffect(() => {
    if (pinnedRef.current && scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [logs]);

  const handleScroll = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    pinnedRef.current = distanceFromBottom < 24;
  }, []);

  const handleClear = useCallback(() => {
    pendingRef.current = [];
    setLogs([]);
    pinnedRef.current = true;
  }, []);

  const handleCopy = useCallback(async () => {
    const text = logs
      .map((l) => `${l.time} ${metaFor(l.level).tag} ${l.message}`)
      .join("\n");
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch (error) {
      console.error("Failed to copy logs:", error);
    }
  }, [logs]);

  return (
    <SettingContainer
      title={t("settings.debug.liveLogs.title")}
      description={t("settings.debug.liveLogs.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
      layout="stacked"
    >
      <div className="flex items-center justify-between mb-2 gap-2">
        <div className="flex items-center gap-2 text-xs text-mid-gray min-w-0">
          <span
            className={`inline-block w-2 h-2 rounded-full shrink-0 ${
              paused ? "bg-mid-gray" : "bg-emerald-500 animate-pulse"
            }`}
          />
          <span className="shrink-0">
            {paused
              ? t("settings.debug.liveLogs.paused")
              : t("settings.debug.liveLogs.live")}
          </span>
          <span className="shrink-0">·</span>
          <span className="truncate">
            {t("settings.debug.liveLogs.lineCount", { count: logs.length })}
          </span>
        </div>
        <div className="flex items-center gap-2 shrink-0">
          <Button
            variant="secondary"
            size="sm"
            onClick={() => setPaused((p) => !p)}
          >
            {paused
              ? t("settings.debug.liveLogs.resume")
              : t("settings.debug.liveLogs.pause")}
          </Button>
          <Button
            variant="secondary"
            size="sm"
            onClick={handleCopy}
            disabled={logs.length === 0}
          >
            {copied ? t("settings.debug.liveLogs.copied") : t("common.copy")}
          </Button>
          <Button
            variant="secondary"
            size="sm"
            onClick={handleClear}
            disabled={logs.length === 0}
          >
            {t("common.clear")}
          </Button>
        </div>
      </div>

      <div
        ref={scrollRef}
        onScroll={handleScroll}
        className="h-72 overflow-y-auto rounded-lg border border-mid-gray/30 bg-[var(--color-log-surface)] p-3 font-mono text-xs leading-relaxed select-text"
      >
        {logs.length === 0 ? (
          <div className="text-mid-gray select-none">
            {t("settings.debug.liveLogs.empty")}
          </div>
        ) : (
          logs.map((line) => {
            const meta = metaFor(line.level);
            return (
              <div key={line.id} className="flex gap-2">
                <span className="text-mid-gray/80 shrink-0 select-none tabular-nums">
                  {line.time}
                </span>
                <span
                  className={`${meta.tagClass} shrink-0 select-none w-[3.5rem]`}
                >
                  {meta.tag}
                </span>
                <span
                  className={`${meta.msgClass} min-w-0 whitespace-pre-wrap break-words`}
                >
                  {line.message}
                </span>
              </div>
            );
          })
        )}
      </div>
    </SettingContainer>
  );
};
