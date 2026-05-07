import { listen } from "@tauri-apps/api/event";
import React, {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import {
  MicrophoneIcon,
  TranscriptionIcon,
  CancelIcon,
} from "../components/icons";
import "./RecordingOverlay.css";
import { commands } from "@/bindings";
import i18n, { syncLanguageFromSettings } from "@/i18n";
import { getLanguageDirection } from "@/lib/utils/rtl";

type OverlayState = "recording" | "transcribing" | "processing";
type LiveSttTranscriptStatus = "partial" | "final" | "error" | "ended";

type LiveSttTranscriptPayload = {
  session_id: number;
  text: string;
};

type LiveSttErrorPayload = {
  session_id?: number | null;
  error_code: string;
  error_message: string;
};

const LIVESTT_FINAL_DISPLAY_DELAY_MS = 140;

const mergeLiveSttFinalText = (confirmedText: string, nextFinal: string) => {
  if (!nextFinal.trim()) {
    return confirmedText;
  }

  const existing = confirmedText.trim();
  const nextWithoutOuterPadding = nextFinal.trim();

  if (!existing) {
    return nextFinal.trimStart();
  }

  if (nextWithoutOuterPadding.startsWith(existing)) {
    return nextWithoutOuterPadding;
  }

  return `${confirmedText}${nextFinal}`;
};

const composeLiveSttDisplayText = (
  confirmedText: string,
  partialText: string,
) => {
  if (!partialText.trim()) {
    return confirmedText;
  }

  if (!confirmedText.trim()) {
    return partialText.trimStart();
  }

  return `${confirmedText}${partialText}`;
};

const RecordingOverlay: React.FC = () => {
  const { t } = useTranslation();
  const [isVisible, setIsVisible] = useState(false);
  const [state, setState] = useState<OverlayState>("recording");
  const [levels, setLevels] = useState<number[]>(Array(16).fill(0));
  const [liveSttText, setLiveSttText] = useState("");
  const [liveSttStatus, setLiveSttStatus] =
    useState<LiveSttTranscriptStatus>("ended");
  const smoothedLevelsRef = useRef<number[]>(Array(16).fill(0));
  const liveSttConfirmedTextRef = useRef("");
  const liveSttTextRef = useRef<HTMLDivElement | null>(null);
  const liveSttFinalDisplayTimerRef = useRef<ReturnType<
    typeof setTimeout
  > | null>(null);
  const direction = getLanguageDirection(i18n.language);

  const clearLiveSttFinalDisplayTimer = useCallback(() => {
    if (liveSttFinalDisplayTimerRef.current) {
      clearTimeout(liveSttFinalDisplayTimerRef.current);
      liveSttFinalDisplayTimerRef.current = null;
    }
  }, []);

  useLayoutEffect(() => {
    const textElement = liveSttTextRef.current;
    if (!textElement) {
      return;
    }

    textElement.scrollTop = textElement.scrollHeight;
  }, [liveSttText]);

  useEffect(() => {
    let isMounted = true;
    let cleanup: (() => void) | undefined;

    const setupEventListeners = async () => {
      const unlistenShow = await listen("show-overlay", async (event) => {
        await syncLanguageFromSettings();
        const overlayState = event.payload as OverlayState;
        setState(overlayState);
        setIsVisible(true);
      });

      const unlistenHide = await listen("hide-overlay", () => {
        setIsVisible(false);
        clearLiveSttFinalDisplayTimer();
        liveSttConfirmedTextRef.current = "";
        setLiveSttText("");
        setLiveSttStatus("ended");
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

      const unlistenLiveSttStarted = await listen(
        "livestt-session-started",
        () => {
          clearLiveSttFinalDisplayTimer();
          liveSttConfirmedTextRef.current = "";
          setLiveSttText("");
          setLiveSttStatus("partial");
        },
      );

      const unlistenLiveSttPartial = await listen<LiveSttTranscriptPayload>(
        "livestt-partial",
        (event) => {
          clearLiveSttFinalDisplayTimer();
          setLiveSttText(
            composeLiveSttDisplayText(
              liveSttConfirmedTextRef.current,
              event.payload.text,
            ),
          );
          setLiveSttStatus("partial");
        },
      );

      const unlistenLiveSttFinal = await listen<LiveSttTranscriptPayload>(
        "livestt-final",
        (event) => {
          const confirmedText = mergeLiveSttFinalText(
            liveSttConfirmedTextRef.current,
            event.payload.text,
          );
          liveSttConfirmedTextRef.current = confirmedText;
          clearLiveSttFinalDisplayTimer();
          liveSttFinalDisplayTimerRef.current = setTimeout(() => {
            liveSttFinalDisplayTimerRef.current = null;
            setLiveSttText(confirmedText);
            setLiveSttStatus("final");
          }, LIVESTT_FINAL_DISPLAY_DELAY_MS);
        },
      );

      const unlistenLiveSttError = await listen<LiveSttErrorPayload>(
        "livestt-error",
        (event) => {
          clearLiveSttFinalDisplayTimer();
          const message =
            event.payload.error_message?.trim() || event.payload.error_code;
          setLiveSttText(
            t("overlay.livesttError", {
              code: event.payload.error_code,
              message,
            }),
          );
          setLiveSttStatus("error");
        },
      );

      const unlistenLiveSttEnded = await listen("livestt-session-ended", () => {
        clearLiveSttFinalDisplayTimer();
        setLiveSttText(liveSttConfirmedTextRef.current);
        setLiveSttStatus("ended");
      });

      cleanup = () => {
        unlistenShow();
        unlistenHide();
        unlistenLevel();
        unlistenLiveSttStarted();
        unlistenLiveSttPartial();
        unlistenLiveSttFinal();
        unlistenLiveSttError();
        unlistenLiveSttEnded();
      };

      if (!isMounted) {
        cleanup();
      }
    };

    setupEventListeners();

    return () => {
      isMounted = false;
      clearLiveSttFinalDisplayTimer();
      cleanup?.();
    };
  }, [clearLiveSttFinalDisplayTimer, t]);

  const getIcon = () => {
    if (state === "recording") {
      return <MicrophoneIcon />;
    } else {
      return <TranscriptionIcon />;
    }
  };

  return (
    <div
      dir={direction}
      className={`recording-overlay ${isVisible ? "fade-in" : ""}`}
    >
      <div className="overlay-left">{getIcon()}</div>

      <div className="overlay-middle">
        {liveSttText ? (
          <div
            ref={liveSttTextRef}
            className={`livestt-text livestt-text-${liveSttStatus}`}
          >
            {liveSttText}
          </div>
        ) : state === "recording" ? (
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
        ) : null}
        {!liveSttText && state === "transcribing" && (
          <div className="transcribing-text">{t("overlay.transcribing")}</div>
        )}
        {!liveSttText && state === "processing" && (
          <div className="transcribing-text">{t("overlay.processing")}</div>
        )}
      </div>

      <div className="overlay-right">
        {state === "recording" && (
          <div
            className="cancel-button"
            onClick={() => {
              commands.cancelOperation();
            }}
          >
            <CancelIcon />
          </div>
        )}
      </div>
    </div>
  );
};

export default RecordingOverlay;
