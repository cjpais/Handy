import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { commands, type ModelStatus as ModelStatusData } from "@/bindings";
import { Button } from "./ui/Button";
import { Spinner } from "./ui/Spinner";

interface DownloadProgress {
  downloaded: number;
  total: number;
  percentage: number;
}

/**
 * The one speech model: download progress while it downloads, then loading,
 * then ready. A failed download offers a retry.
 */
export const ModelStatus: React.FC = () => {
  const { t } = useTranslation();
  const [status, setStatus] = useState<ModelStatusData | null>(null);
  const [percent, setPercent] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setStatus(await commands.getModelStatus());
  }, []);

  useEffect(() => {
    // Listeners first, so a change between the listen and the first read is
    // not lost.
    const unlisteners = [
      listen<DownloadProgress>("model-download-progress", (event) => {
        setError(null);
        setPercent(event.payload.percentage);
        setStatus((prev) => (prev ? { ...prev, is_downloading: true } : prev));
      }),
      listen("model-download-complete", () => {
        setPercent(null);
        refresh();
      }),
      listen<string>("model-download-failed", (event) => {
        setPercent(null);
        setError(event.payload);
        refresh();
      }),
      listen("model-download-cancelled", () => {
        setPercent(null);
        refresh();
      }),
      listen("model-state-changed", () => {
        refresh();
      }),
    ];
    refresh();
    return () => {
      unlisteners.forEach((p) => p.then((off) => off()));
    };
  }, [refresh]);

  if (!status) return null;

  const retry = () => {
    setError(null);
    commands.downloadModel();
  };

  let state: React.ReactNode;
  if (status.is_loaded) {
    state = <span className="text-moss">{t("model.ready")}</span>;
  } else if (status.is_downloading) {
    state = (
      <div className="flex items-center gap-3 w-full max-w-xs">
        <div className="h-1.5 flex-1 rounded-full bg-wash overflow-hidden">
          <div
            className="h-full bg-logo-primary transition-[width]"
            style={{ width: `${percent ?? 0}%` }}
          />
        </div>
        <span className="tabular-nums text-ink-2">
          {t("model.downloading", { percent: Math.round(percent ?? 0) })}
        </span>
        <Button
          variant="secondary"
          size="sm"
          onClick={() => commands.cancelDownload()}
        >
          {t("model.cancel")}
        </Button>
      </div>
    );
  } else if (status.is_downloaded) {
    state = (
      <span className="inline-flex items-center gap-2 text-ink-2">
        <Spinner className="shrink-0" />
        {t("model.loading")}
      </span>
    );
  } else {
    state = (
      <span className="inline-flex items-center gap-3">
        <span className="text-ink-2">
          {error ? t("model.failed") : t("model.missing")}
        </span>
        <Button variant="primary" size="sm" onClick={retry}>
          {error ? t("model.retry") : t("model.download")}
        </Button>
      </span>
    );
  }

  return (
    <div className="w-full rounded-lg border border-line px-4 py-3 flex items-center justify-between gap-4 text-sm">
      <div>
        <b className="block font-semibold">{status.name}</b>
        <span className="text-ink-3">
          {t("model.size", { size: status.size_mb })}
        </span>
      </div>
      {state}
    </div>
  );
};
