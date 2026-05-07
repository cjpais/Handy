import React, { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import type { LogEntry } from "@/bindings";
import { useLogStream } from "./useLogStream";

const LEVEL_TEXT: Record<string, string> = {
  error: "text-red-500",
  warn: "text-amber-500",
  info: "text-sky-600",
  debug: "text-slate-500",
  trace: "text-slate-400",
};

const LEVEL_BG: Record<string, string> = {
  error: "bg-red-50",
  warn: "bg-amber-50",
  info: "",
  debug: "",
  trace: "",
};

const SOURCE_BADGE: Record<string, string> = {
  rust: "bg-orange-100 text-orange-700",
  front: "bg-indigo-100 text-indigo-700",
};

const LEVELS = ["error", "warn", "info", "debug", "trace"] as const;

function formatTime(tsMs: number): string {
  const d = new Date(tsMs);
  const h = String(d.getHours()).padStart(2, "0");
  const m = String(d.getMinutes()).padStart(2, "0");
  const s = String(d.getSeconds()).padStart(2, "0");
  const ms = String(d.getMilliseconds()).padStart(3, "0");
  return `${h}:${m}:${s}.${ms}`;
}

function LogRow({ entry }: { entry: LogEntry }) {
  const levelText = LEVEL_TEXT[entry.level] ?? "text-slate-500";
  const rowBg = LEVEL_BG[entry.level] ?? "";
  const badgeCls = SOURCE_BADGE[entry.source] ?? "bg-slate-100 text-slate-600";

  return (
    <div
      className={`grid min-w-0 grid-cols-[90px_44px_auto_minmax(96px,150px)_minmax(0,1fr)] items-baseline gap-2 px-3 py-[3px] font-mono text-[11px] leading-[18px] hover:bg-slate-50 ${rowBg}`}
      title={entry.message}
    >
      <span className="tabular-nums text-slate-400">
        {formatTime(entry.ts_ms)}
      </span>
      <span className={`font-semibold uppercase ${levelText}`}>
        {entry.level.slice(0, 5)}
      </span>
      <span
        className={`w-fit rounded px-1 py-px text-[9px] font-semibold leading-3 ${badgeCls}`}
      >
        {entry.source}
      </span>
      <span className="min-w-0 truncate text-slate-400">{entry.target}</span>
      <span className="min-w-0 truncate text-slate-700">{entry.message}</span>
    </div>
  );
}

export const LogConsole: React.FC = () => {
  const { t } = useTranslation();
  const {
    entries,
    totalCount,
    paused,
    setPaused,
    levelFilter,
    setLevelFilter,
    textFilter,
    setTextFilter,
    clear,
    copyAll,
  } = useLogStream();

  const scrollRef = useRef<HTMLDivElement>(null);
  const prevTotalRef = useRef(0);

  useEffect(() => {
    if (!paused && totalCount > prevTotalRef.current) {
      const el = scrollRef.current;
      if (el) el.scrollTop = el.scrollHeight;
    }
    prevTotalRef.current = totalCount;
  }, [totalCount, paused]);

  const toggleLevel = (level: string) => {
    setLevelFilter((prev) => ({ ...prev, [level]: !prev[level] }));
  };

  return (
    <div className="flex w-full min-w-0 flex-col gap-3 p-4">
      {/* Toolbar row 1: level toggles + text filter */}
      <div className="flex min-w-0 flex-wrap items-center gap-2">
        <div className="flex min-w-0 flex-wrap items-center gap-1 rounded-xl bg-slate-100 p-1">
          {LEVELS.map((level) => {
            const active = levelFilter[level] ?? true;
            return (
              <button
                key={level}
                onClick={() => toggleLevel(level)}
                className={`h-7 rounded-lg px-2.5 text-[11px] font-semibold capitalize transition-colors ${
                  active
                    ? `${LEVEL_TEXT[level]} bg-white shadow-sm`
                    : "text-slate-300 hover:text-slate-400"
                }`}
              >
                {level}
              </button>
            );
          })}
        </div>
        <input
          type="text"
          value={textFilter}
          onChange={(e) => setTextFilter(e.target.value)}
          placeholder={t("settings.debug.logs.filter.text.placeholder")}
          className="h-8 min-w-[180px] flex-1 rounded-lg border border-slate-200 bg-white px-3 text-xs outline-none focus:border-slate-400"
        />
      </div>

      {/* Toolbar row 2: action buttons */}
      <div className="flex flex-wrap items-center justify-end gap-2">
        <button
          onClick={() => setPaused((p) => !p)}
          className={`h-8 whitespace-nowrap rounded-lg px-3 text-xs font-medium transition-colors ${
            paused
              ? "bg-amber-100 text-amber-700 hover:bg-amber-200"
              : "bg-slate-100 text-slate-500 hover:bg-slate-200"
          }`}
        >
          {paused
            ? t("settings.debug.logs.paused")
            : t("settings.debug.logs.pause")}
        </button>
        <button
          onClick={clear}
          className="h-8 whitespace-nowrap rounded-lg px-3 text-xs font-medium text-slate-500 hover:bg-slate-100"
        >
          {t("settings.debug.logs.clear")}
        </button>
        <button
          onClick={copyAll}
          className="h-8 whitespace-nowrap rounded-lg px-3 text-xs font-medium text-slate-500 hover:bg-slate-100"
        >
          {t("settings.debug.logs.copyAll")}
        </button>
        <button
          onClick={() => commands.openLogDir()}
          className="h-8 whitespace-nowrap rounded-lg px-3 text-xs font-medium text-slate-500 hover:bg-slate-100"
        >
          {t("settings.debug.logs.openLogDir")}
        </button>
      </div>

      {/* Log area */}
      <div
        ref={scrollRef}
        className="h-[clamp(240px,42vh,520px)] min-w-0 overflow-y-auto overflow-x-hidden rounded-xl border border-slate-200 bg-white py-1"
      >
        {entries.length === 0 ? (
          <p className="px-3 py-6 text-center font-mono text-xs text-slate-400">
            —
          </p>
        ) : (
          entries.map((entry, i) => (
            <LogRow key={`${entry.ts_ms}-${i}`} entry={entry} />
          ))
        )}
      </div>

      <p className="text-right font-mono text-[10px] text-slate-400">
        {entries.length} / {totalCount}
      </p>
    </div>
  );
};
