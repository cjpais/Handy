import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { commands } from "@/bindings";
import { useModelStore } from "@/stores/modelStore";

/** Download id of the language detector (`LANGUAGE_DETECTOR_ID` in Rust). */
export const LANGUAGE_DETECTOR_ID = "language-detector";

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
    // Failure toasts from the model store's download-failed listener.
    await commands.downloadLanguageDetector();
    setStarting(false);
    await refresh();
  }, [refresh]);

  return {
    downloaded,
    downloading: starting || progress !== undefined,
    percent: Math.round(progress?.percentage ?? 0),
    download,
  };
};
