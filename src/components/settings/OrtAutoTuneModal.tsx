import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { commands, type BenchmarkProgress, type BenchmarkResult } from "@/bindings";
import { Button } from "../ui/Button";

interface OrtAutoTuneModalProps {
  isOpen: boolean;
  onClose: () => void;
  onApply: (threadCount: number) => void;
}

type Phase = "running" | "complete" | "error";

export const OrtAutoTuneModal: React.FC<OrtAutoTuneModalProps> = ({
  isOpen,
  onClose,
  onApply,
}) => {
  const { t } = useTranslation();
  const [phase, setPhase] = useState<Phase>("running");
  const [progress, setProgress] = useState<BenchmarkProgress | null>(null);
  const [result, setResult] = useState<BenchmarkResult | null>(null);
  const [errorMessage, setErrorMessage] = useState<string>("");
  const unlistenRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    if (!isOpen) {
      return;
    }

    setPhase("running");
    setProgress(null);
    setResult(null);
    setErrorMessage("");

    let cancelled = false;

    const run = async () => {
      const unlisten = await listen<BenchmarkProgress>(
        "ort-benchmark-progress",
        (event) => {
          if (!cancelled) {
            setProgress(event.payload);
          }
        },
      );
      unlistenRef.current = unlisten;
      // Guard against unmount during the await above
      if (cancelled) {
        unlisten();
        unlistenRef.current = null;
        return;
      }

      const response = await commands.benchmarkOrtThreadCount();
      if (cancelled) {
        return;
      }
      if (response.status === "ok") {
        if (response.data.cancelled) {
          onClose();
        } else {
          setResult(response.data);
          setPhase("complete");
        }
      } else {
        setErrorMessage(response.error);
        setPhase("error");
      }
    };

    run().catch((err) => {
      if (!cancelled) {
        setErrorMessage(err instanceof Error ? err.message : String(err));
        setPhase("error");
      }
    });

    return () => {
      cancelled = true;
      if (unlistenRef.current) {
        unlistenRef.current();
        unlistenRef.current = null;
      }
    };
  }, [isOpen]);

  if (!isOpen) {
    return null;
  }

  const handleApply = () => {
    if (result) {
      onApply(result.best_thread_count);
    }
    onClose();
  };

  const handleCancel = async () => {
    if (phase === "running") {
      await commands.cancelBenchmark();
    }
    onClose();
  };

  const progressPercent =
    progress && progress.total > 0
      ? Math.round((progress.trial / progress.total) * 100)
      : 0;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="w-full max-w-md rounded-lg border border-mid-gray/20 bg-background p-6 shadow-xl">
        <h2 className="mb-4 text-base font-semibold text-text">
          {t("settings.advanced.ortThreadCount.benchmarkTitle")}
        </h2>

        {phase === "running" && (
          <div className="space-y-4">
            <p className="text-sm text-text/70">
              {progress
                ? t("settings.advanced.ortThreadCount.benchmarkProgress", {
                    threads: progress.thread_count,
                    current: progress.trial,
                    total: progress.total,
                  })
                : t("settings.advanced.ortThreadCount.benchmarkStarting")}
            </p>
            <div className="h-2 w-full overflow-hidden rounded-full bg-mid-gray/20">
              <div
                className="h-full rounded-full bg-logo-primary transition-all duration-300"
                style={{ width: `${progressPercent}%` }}
              />
            </div>
          </div>
        )}

        {phase === "complete" && result && (
          <div className="space-y-3">
            <p className="text-sm font-medium text-text">
              {t("settings.advanced.ortThreadCount.benchmarkComplete", {
                threads: result.best_thread_count,
                time: (result.best_time_ms / 1000).toFixed(1),
                duration: (result.audio_duration_ms / 1000).toFixed(1),
              })}
            </p>
            {result.all_timings.length > 0 && (
              <div className="max-h-40 overflow-y-auto rounded border border-mid-gray/20 text-xs">
                <table className="w-full">
                  <thead>
                    <tr className="border-b border-mid-gray/20 text-text/60">
                      <th className="px-2 py-1 text-left font-medium">{t("settings.advanced.ortThreadCount.benchmarkThreads")}</th>
                      <th className="px-2 py-1 text-right font-medium">{t("settings.advanced.ortThreadCount.benchmarkTime")}</th>
                      <th className="px-2 py-1 text-right font-medium">{t("settings.advanced.ortThreadCount.benchmarkRtf")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {result.all_timings.map((t) => {
                      const isBest = t.thread_count === result.best_thread_count;
                      const rtf = result.audio_duration_ms > 0
                        ? (result.audio_duration_ms / t.elapsed_ms).toFixed(1)
                        : "—";
                      return (
                        <tr
                          key={t.thread_count}
                          className={isBest ? "bg-logo-primary/10 font-semibold text-text" : "text-text/70"}
                        >
                          <td className="px-2 py-0.5">
                            {t.thread_count === 0 ? "auto" : t.thread_count}
                            {isBest ? " ★" : ""}
                          </td>
                          <td className="px-2 py-0.5 text-right">
                            {`${(t.elapsed_ms / 1000).toFixed(1)}s`}
                          </td>
                          <td className="px-2 py-0.5 text-right">{`${rtf}x`}</td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            )}
            <p className="text-xs text-text/60">
              {t("settings.advanced.ortThreadCount.benchmarkModel", {
                model: result.model_name,
              })}
            </p>
            <p className="text-xs text-text/50">
              {t("settings.advanced.ortThreadCount.benchmarkNote")}
            </p>
          </div>
        )}

        {phase === "error" && (
          <div className="space-y-2">
            <p className="text-sm font-medium text-red-400">
              {t("settings.advanced.ortThreadCount.benchmarkError")}
            </p>
            <p className="text-xs text-text/60">{errorMessage}</p>
          </div>
        )}

        <div className="mt-6 flex justify-end gap-2">
          {phase === "complete" && (
            <Button variant="primary" size="sm" onClick={handleApply}>
              {t("settings.advanced.ortThreadCount.apply")}
            </Button>
          )}
          <Button variant="secondary" size="sm" onClick={handleCancel}>
            {t("settings.advanced.ortThreadCount.cancel")}
          </Button>
        </div>
      </div>
    </div>
  );
};
