import { useState, useEffect, useCallback } from "react";
import { commands } from "@/bindings";
import { listen } from "@tauri-apps/api/event";

interface VadDownloadProgress {
  downloaded: number;
  total: number;
  percentage: number;
}

interface UseVadModelReturn {
  isReady: boolean;
  isDownloading: boolean;
  downloadProgress: number;
  error: string | null;
  checkAndDownload: () => Promise<void>;
}

export const useVadModel = (): UseVadModelReturn => {
  const [isReady, setIsReady] = useState(false);
  const [isDownloading, setIsDownloading] = useState(false);
  const [downloadProgress, setDownloadProgress] = useState(0);
  const [error, setError] = useState<string | null>(null);

  // Check if VAD model is ready on mount
  useEffect(() => {
    const checkReady = async () => {
      try {
        const ready = await commands.isVadModelReady();
        setIsReady(ready);
      } catch (e) {
        console.error("Failed to check VAD model status:", e);
      }
    };
    checkReady();
  }, []);

  // Listen for download events
  useEffect(() => {
    const unlistenProgress = listen<VadDownloadProgress>(
      "vad-download-progress",
      (event) => {
        setDownloadProgress(event.payload.percentage);
      }
    );

    const unlistenComplete = listen("vad-download-complete", () => {
      setIsDownloading(false);
      setIsReady(true);
      setDownloadProgress(100);
    });

    const unlistenFailed = listen<string>("vad-download-failed", (event) => {
      setIsDownloading(false);
      setError(event.payload);
    });

    const unlistenStarted = listen("vad-download-started", () => {
      setIsDownloading(true);
      setDownloadProgress(0);
      setError(null);
    });

    return () => {
      unlistenProgress.then((fn) => fn());
      unlistenComplete.then((fn) => fn());
      unlistenFailed.then((fn) => fn());
      unlistenStarted.then((fn) => fn());
    };
  }, []);

  const checkAndDownload = useCallback(async () => {
    try {
      setError(null);
      const result = await commands.downloadVadModelIfNeeded();
      if (result.status === "ok") {
        setIsReady(true);
      } else {
        setError(result.error);
      }
    } catch (e) {
      setError(String(e));
    }
  }, []);

  return {
    isReady,
    isDownloading,
    downloadProgress,
    error,
    checkAndDownload,
  };
};
