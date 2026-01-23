import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

export interface TranscriptionError {
  type: "local" | "missing_api_key" | "cloud_api" | "network" | "no_provider";
  message: string;
}

const AUTO_CLEAR_DELAY_MS = 10000;

export function useTranscriptionErrors() {
  const [error, setError] = useState<TranscriptionError | null>(null);
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    const unlisten = listen<TranscriptionError>(
      "transcription-error",
      (event) => {
        // Clear any existing timeout before setting a new one
        if (timeoutRef.current) {
          clearTimeout(timeoutRef.current);
        }

        setError(event.payload);

        timeoutRef.current = setTimeout(() => {
          setError(null);
          timeoutRef.current = null;
        }, AUTO_CLEAR_DELAY_MS);
      },
    );

    return () => {
      unlisten.then((fn) => fn());
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current);
      }
    };
  }, []);

  const clearError = () => {
    if (timeoutRef.current) {
      clearTimeout(timeoutRef.current);
      timeoutRef.current = null;
    }
    setError(null);
  };

  return { error, clearError };
}
