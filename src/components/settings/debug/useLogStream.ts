import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { commands } from "@/bindings";
import type { LogEntry } from "@/bindings";

export type LevelFilter = Record<string, boolean>;

const DEFAULT_LEVELS: LevelFilter = {
  error: true,
  warn: true,
  info: true,
  debug: true,
  trace: true,
};

declare global {
  interface Window {
    __logStreamPatched?: boolean;
  }
}

// Module-level subscribers for console-patched entries
let frontendEntryListeners: Array<(entry: LogEntry) => void> = [];

function patchConsole() {
  if (typeof window === "undefined" || window.__logStreamPatched) return;
  window.__logStreamPatched = true;

  const methods = ["log", "info", "warn", "error", "debug"] as const;
  const levelMap: Record<string, string> = {
    log: "info",
    info: "info",
    warn: "warn",
    error: "error",
    debug: "debug",
  };

  for (const method of methods) {
    const original = console[method].bind(console);
    (console as unknown as Record<string, (...a: unknown[]) => void>)[method] =
      (...args: unknown[]) => {
        original(...args);
        const entry: LogEntry = {
          ts_ms: Date.now(),
          level: levelMap[method],
          target: "frontend",
          message: args
            .map((a) =>
              typeof a === "string" ? a : JSON.stringify(a, null, 0),
            )
            .join(" "),
          source: "front",
        };
        for (const cb of frontendEntryListeners) cb(entry);
      };
  }
}

export function useLogStream() {
  const [allEntries, setAllEntries] = useState<LogEntry[]>([]);
  const [paused, setPaused] = useState(false);
  const [levelFilter, setLevelFilter] = useState<LevelFilter>(DEFAULT_LEVELS);
  const [textFilter, setTextFilter] = useState("");
  const pausedRef = useRef(paused);
  pausedRef.current = paused;

  // Patch console once globally
  useEffect(() => {
    patchConsole();
  }, []);

  useEffect(() => {
    // Backfill from Rust ring buffer on mount
    commands.getLogBuffer().then((data) => {
      setAllEntries((prev) => {
        const merged = [...data, ...prev].sort((a, b) => a.ts_ms - b.ts_ms);
        return merged.slice(-2000);
      });
    });

    // Subscribe to Rust log events via Tauri event
    const unlistenRust = listen<LogEntry>("app://log", (event) => {
      if (!pausedRef.current) {
        setAllEntries((prev) => {
          const next = [...prev, event.payload];
          return next.length > 2000 ? next.slice(-2000) : next;
        });
      }
    });

    // Subscribe to console-patched frontend entries
    const onFrontendEntry = (entry: LogEntry) => {
      if (!pausedRef.current) {
        setAllEntries((prev) => {
          const next = [...prev, entry];
          return next.length > 2000 ? next.slice(-2000) : next;
        });
      }
    };
    frontendEntryListeners.push(onFrontendEntry);

    return () => {
      unlistenRust.then((fn) => fn());
      frontendEntryListeners = frontendEntryListeners.filter(
        (l) => l !== onFrontendEntry,
      );
    };
  }, []);

  const clear = useCallback(async () => {
    setAllEntries([]);
    await commands.clearLogBuffer().catch(() => void 0);
  }, []);

  const filtered = allEntries.filter((e) => {
    if (!levelFilter[e.level]) return false;
    if (textFilter) {
      const q = textFilter.toLowerCase();
      return (
        e.message.toLowerCase().includes(q) ||
        e.target.toLowerCase().includes(q)
      );
    }
    return true;
  });

  const copyAll = useCallback(async () => {
    const text = filtered
      .map((e) => {
        const d = new Date(e.ts_ms);
        const hms = d.toTimeString().slice(0, 8);
        const ms = String(d.getMilliseconds()).padStart(3, "0");
        return `[${hms}.${ms}] ${e.level.toUpperCase().padEnd(5)} [${e.source}] ${e.target} — ${e.message}`;
      })
      .join("\n");
    await navigator.clipboard.writeText(text);
  }, [filtered]);

  return {
    entries: filtered,
    totalCount: allEntries.length,
    paused,
    setPaused,
    levelFilter,
    setLevelFilter,
    textFilter,
    setTextFilter,
    clear,
    copyAll,
  };
}
