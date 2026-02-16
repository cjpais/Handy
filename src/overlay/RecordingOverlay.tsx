import { listen } from "@tauri-apps/api/event";
import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  MicrophoneIcon,
  TranscriptionIcon,
  CancelIcon,
  CopyIcon,
  CheckIcon,
} from "../components/icons";
import "./RecordingOverlay.css";
import { commands } from "@/bindings";
import i18n, { syncLanguageFromSettings } from "@/i18n";
import { getLanguageDirection } from "@/lib/utils/rtl";

type OverlayState = "recording" | "transcribing" | "processing" | "done";

const RecordingOverlay: React.FC = () => {
  const { t } = useTranslation();
  const [isVisible, setIsVisible] = useState(false);
  const [state, setState] = useState<OverlayState>("recording");
  const [levels, setLevels] = useState<number[]>(Array(16).fill(0));
  const [streamingText, setStreamingText] = useState<string>("");
  const [copied, setCopied] = useState(false);
  const smoothedLevelsRef = useRef<number[]>(Array(16).fill(0));
  const copiedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const direction = getLanguageDirection(i18n.language);

  useEffect(() => {
    const setupEventListeners = async () => {
      const unlistenShow = await listen("show-overlay", async (event) => {
        await syncLanguageFromSettings();
        const overlayState = event.payload as OverlayState;
        if (overlayState === "recording") {
          smoothedLevelsRef.current = Array(16).fill(0);
          setLevels(Array(9).fill(0));
          setStreamingText("");
          setCopied(false);
        }
        setState(overlayState);
        setIsVisible(true);
      });

      const unlistenHide = await listen("hide-overlay", () => {
        setIsVisible(false);
        setState("recording");
        smoothedLevelsRef.current = Array(16).fill(0);
        setLevels(Array(9).fill(0));
        setStreamingText("");
        setCopied(false);
      });

      const unlistenLevel = await listen<number[]>("mic-level", (event) => {
        const newLevels = event.payload as number[];
        const smoothed = smoothedLevelsRef.current.map((prev, i) => {
          const target = newLevels[i] || 0;
          return prev * 0.7 + target * 0.3;
        });
        smoothedLevelsRef.current = smoothed;
        setLevels(smoothed.slice(0, 9));
      });

      const unlistenStreaming = await listen<string>(
        "streaming-text",
        (event) => {
          setStreamingText(event.payload);
        },
      );

      const unlistenDone = await listen<string>("overlay-done", (event) => {
        const finalText = event.payload;
        if (finalText) {
          setStreamingText(finalText);
        }
        setState("done");
        setCopied(false);
      });

      return () => {
        unlistenShow();
        unlistenHide();
        unlistenLevel();
        unlistenStreaming();
        unlistenDone();
      };
    };

    setupEventListeners();
  }, []);

  useEffect(() => {
    return () => {
      if (copiedTimerRef.current) {
        clearTimeout(copiedTimerRef.current);
      }
    };
  }, []);

  const handleCopy = async () => {
    if (!streamingText) return;
    try {
      await navigator.clipboard.writeText(streamingText);
      setCopied(true);
      if (copiedTimerRef.current) {
        clearTimeout(copiedTimerRef.current);
      }
      copiedTimerRef.current = setTimeout(() => {
        setCopied(false);
        copiedTimerRef.current = null;
      }, 1500);
    } catch (err) {
      console.error("Failed to copy:", err);
    }
  };

  const handleClose = () => {
    setIsVisible(false);
    setState("recording");
    setStreamingText("");
    setCopied(false);
    commands.cancelOperation();
  };

  const getIcon = () => {
    if (state === "recording") {
      return <MicrophoneIcon />;
    } else {
      return <TranscriptionIcon />;
    }
  };

  const hasStreamingText = streamingText.length > 0;
  const isTranscribing = state === "transcribing" || state === "processing";
  const isDone = state === "done";

  return (
    <div
      dir={direction}
      className={`recording-overlay ${isVisible ? "fade-in" : ""} ${hasStreamingText ? "has-text" : ""} ${isDone ? "done" : ""}`}
    >
      <div className="overlay-left">{getIcon()}</div>

      <div className="overlay-middle">
        {state === "recording" && !hasStreamingText && (
          <div className="bars-container">
            {levels.map((v, i) => (
              <div
                key={i}
                className="bar"
                style={{
                  height: `${Math.min(20, 4 + Math.pow(v, 0.7) * 16)}px`,
                  transition: "height 60ms ease-out, opacity 120ms ease-out",
                  opacity: Math.max(0.2, v * 1.7),
                }}
              />
            ))}
          </div>
        )}
        {hasStreamingText && (
          <div
            className={`streaming-text ${isTranscribing ? "processing" : ""}`}
          >
            {streamingText}
          </div>
        )}
        {isTranscribing && !hasStreamingText && (
          <div className="transcribing-text">
            {state === "processing"
              ? t("overlay.processing")
              : t("overlay.transcribing")}
          </div>
        )}
      </div>

      <div className="overlay-right">
        {hasStreamingText ? (
          <div className="done-buttons">
            {isTranscribing && <div className="processing-indicator" />}
            <div
              className="overlay-btn copy-button"
              onClick={handleCopy}
              title="Copy"
            >
              {copied ? (
                <CheckIcon width={16} height={16} />
              ) : (
                <CopyIcon width={16} height={16} />
              )}
            </div>
            <div
              className="overlay-btn close-button"
              onClick={handleClose}
              title="Close"
            >
              <CancelIcon width={18} height={18} />
            </div>
          </div>
        ) : (
          state === "recording" && (
            <div
              className="cancel-button"
              onClick={() => {
                commands.cancelOperation();
              }}
            >
              <CancelIcon />
            </div>
          )
        )}
      </div>
    </div>
  );
};

export default RecordingOverlay;
