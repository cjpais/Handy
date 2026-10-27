import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { commands } from "@/bindings";
import { LANGUAGE_DETECTION_MODEL, modelFile } from "@handy-computer/models";
import { useModelStore } from "@/stores/modelStore";
import { BYTES_PER_MB, formatModelSize } from "@/lib/utils/format";

/** Download id of the language detector (`LANGUAGE_DETECTOR_ID` in Rust). */
export const LANGUAGE_DETECTOR_ID = "language-detector";

/** The size of the detector file the backend fetches. */
export const LANGUAGE_DETECTOR_SIZE = formatModelSize(
  modelFile(LANGUAGE_DETECTION_MODEL).size / BYTES_PER_MB,
);

/** The language detector's download state, shared everywhere it's offered. */
export const useLanguageDetector = () => {
  const [downloaded, setDownloaded] = useState<boolean | null>(null);
  const [starting, setStarting] = useState(false);
  const progress = useModelStore(
    (state) => state.downloadProgress[LANGUAGE_DETECTOR_ID],
  );

  const refresh = useCallback(
    () => commands.isLanguageDetectorDownloaded().then(setDownloaded),
    [],
  );

  useEffect(() => {
    refresh();
    // A model download can bring the detector along, so any completion
    // may change it.
    const unlisten = listen("model-download-complete", refresh);
    return () => {
      unlisten.then((stop) => stop());
    };
  }, [refresh]);

  const download = useCallback(async () => {
    setStarting(true);
    try {
      // Failure toasts from the model store's download-failed listener.
      await commands.downloadLanguageDetector();
    } finally {
      setStarting(false);
    }
    await refresh();
  }, [refresh]);

  // Stops at once in the UI; the next download resumes where this one ended.
  const cancel = useCallback(() => {
    setStarting(false);
    return useModelStore.getState().cancelDownload(LANGUAGE_DETECTOR_ID);
  }, []);

  return {
    downloaded,
    downloading: starting || progress !== undefined,
    percent: Math.round(progress?.percentage ?? 0),
    download,
    cancel,
  };
};
