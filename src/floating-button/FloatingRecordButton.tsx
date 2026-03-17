import { listen } from "@tauri-apps/api/event";
import React, { useEffect, useState } from "react";
import { commands } from "@/bindings";
import "./FloatingRecordButton.css";

type ButtonState = "idle" | "recording" | "transcribing" | "processing";

const FloatingRecordButton: React.FC = () => {
  const [state, setState] = useState<ButtonState>("idle");

  useEffect(() => {
    const setupListeners = async () => {
      const unlisten = await listen<string>(
        "floating-button-state",
        (event) => {
          setState(event.payload as ButtonState);
        },
      );

      return () => {
        unlisten();
      };
    };

    setupListeners();
  }, []);

  const handleClick = () => {
    if (state === "idle") {
      setState("recording");
      commands.toggleTranscriptionFromButton();
    } else if (state === "recording") {
      setState("idle");
      commands.toggleTranscriptionFromButton();
    }
  };

  const isRecording = state === "recording";
  const isProcessing = state === "transcribing" || state === "processing";

  return (
    <div
      className={`floating-record-button ${isRecording ? "recording" : ""} ${isProcessing ? "processing" : ""}`}
      onMouseDown={(e) => e.preventDefault()}
      onClick={handleClick}
    >
      {isRecording ? (
        <svg
          width="24"
          height="24"
          viewBox="0 0 24 24"
          fill="currentColor"
          xmlns="http://www.w3.org/2000/svg"
        >
          <rect x="6" y="6" width="12" height="12" rx="2" />
        </svg>
      ) : (
        <svg
          width="24"
          height="24"
          viewBox="0 0 24 24"
          fill="currentColor"
          xmlns="http://www.w3.org/2000/svg"
        >
          <path d="M12 14c1.66 0 3-1.34 3-3V5c0-1.66-1.34-3-3-3S9 3.34 9 5v6c0 1.66 1.34 3 3 3z" />
          <path d="M17 11c0 2.76-2.24 5-5 5s-5-2.24-5-5H5c0 3.53 2.61 6.43 6 6.92V21h2v-3.08c3.39-.49 6-3.39 6-6.92h-2z" />
        </svg>
      )}
    </div>
  );
};

export default FloatingRecordButton;
