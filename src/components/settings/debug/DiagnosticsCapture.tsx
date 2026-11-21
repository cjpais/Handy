import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type DiagnosticsStatus } from "@/bindings";
import { save } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

function traceFileName(): string {
  const now = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  return `handy-trace-${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}.json`;
}

/**
 * Timing-diagnostics capture controls (Debug settings).
 *
 * Starts/stops the trace recorder through the diagnostics commands and
 * saves the finished Chrome Trace Event file for a support session. The
 * trace holds timing data only — never audio or transcribed text.
 */
export const DiagnosticsCapture: React.FC = () => {
  const { t } = useTranslation();
  const [status, setStatus] = useState<DiagnosticsStatus | null>(null);
  const [savedPath, setSavedPath] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const stoppingRef = useRef(false);

  useEffect(() => {
    commands
      .diagnosticsStatus()
      .then(setStatus)
      .catch(() => setStatus(null));
  }, []);

  // Poll event/drop counts once a second, only while a capture is active.
  useEffect(() => {
    if (!status?.active) {
      return;
    }
    const id = window.setInterval(async () => {
      try {
        const next = await commands.diagnosticsStatus();
        if (!stoppingRef.current) {
          setStatus(next);
        }
      } catch {
        // Polling is best-effort; the stop path surfaces real errors.
      }
    }, 1000);
    return () => window.clearInterval(id);
  }, [status?.active]);

  const start = async () => {
    setBusy(true);
    setError(null);
    setSavedPath(null);
    try {
      const result = await commands.startDiagnostics();
      if (result.status === "ok") {
        setStatus(result.data);
      } else {
        setError(result.error);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const stopAndSave = async () => {
    setBusy(true);
    stoppingRef.current = true;
    setError(null);
    try {
      const path = await save({
        defaultPath: traceFileName(),
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      // A cancelled dialog still stops the capture; the file then stays in
      // app data and its path is shown here.
      const result = await commands.stopDiagnostics(path ?? null);
      if (result.status === "ok") {
        setSavedPath(result.data);
        setStatus(await commands.diagnosticsStatus());
      } else {
        setError(result.error);
        setStatus(await commands.diagnosticsStatus());
      }
    } catch (e) {
      setError(String(e));
    } finally {
      stoppingRef.current = false;
      setBusy(false);
    }
  };

  const reveal = async () => {
    if (!savedPath) {
      return;
    }
    try {
      await revealItemInDir(savedPath);
    } catch (e) {
      setError(String(e));
    }
  };

  const active = status?.active ?? false;
  const buttonClass =
    "px-2 py-1 text-sm font-semibold bg-wash border border-line-strong hover:bg-logo-primary/10 rounded cursor-pointer hover:border-logo-primary disabled:opacity-50 disabled:cursor-default whitespace-nowrap";

  return (
    <div className="p-4 space-y-2">
      <div className="flex justify-between items-center gap-2">
        <div>
          <p className="text-sm font-medium">
            {t("settings.debug.diagnostics.title")}
          </p>
          <p className="text-xs text-ink-3">
            {t("settings.debug.diagnostics.description")}
          </p>
        </div>
        {active ? (
          <button onClick={stopAndSave} disabled={busy} className={buttonClass}>
            {t("settings.debug.diagnostics.stopAndSave")}
          </button>
        ) : (
          <button onClick={start} disabled={busy} className={buttonClass}>
            {t("settings.debug.diagnostics.start")}
          </button>
        )}
      </div>
      {active && status && (
        <div className="flex items-center gap-2 text-sm">
          <span className="w-2 h-2 rounded-full bg-error animate-pulse" />
          <span>{t("settings.debug.diagnostics.active")}</span>
          <span className="text-ink-3">
            {t("settings.debug.diagnostics.counts", {
              events: status.events,
              dropped: status.dropped,
            })}
          </span>
        </div>
      )}
      {savedPath && (
        <div className="flex items-center gap-2 text-xs text-ink-3 min-w-0">
          <span className="truncate">
            {t("settings.debug.diagnostics.savedTo", { path: savedPath })}
          </span>
          <button
            onClick={reveal}
            className="underline cursor-pointer hover:text-ink-2 whitespace-nowrap"
          >
            {t("settings.debug.diagnostics.reveal")}
          </button>
        </div>
      )}
      <p className="text-xs text-ink-3">
        {t("settings.debug.diagnostics.privacyNote")}
      </p>
      {error !== null && (
        <p className="text-sm text-error">
          {t("settings.debug.diagnostics.error", { message: error })}
        </p>
      )}
    </div>
  );
};
