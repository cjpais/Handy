import { listen } from "@tauri-apps/api/event";
import React, { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import "./RecordingOverlay.css";
import { commands } from "@/bindings";
import i18n, { syncLanguageFromSettings } from "@/i18n";
import { getLanguageDirection } from "@/lib/utils/rtl";

type OverlayState = "recording" | "transcribing" | "processing";

const MicIcon: React.FC = () => (
  <svg
    width="14"
    height="14"
    viewBox="0 0 24 24"
    fill="none"
    stroke="rgba(255,255,255,0.8)"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" />
    <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
    <line x1="12" x2="12" y1="19" y2="22" />
  </svg>
);

const DotsIcon: React.FC = () => (
  <svg
    width="14"
    height="14"
    viewBox="0 0 24 24"
    fill="none"
    stroke="rgba(255,255,255,0.7)"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
  </svg>
);

const XIcon: React.FC = () => (
  <svg
    width="12"
    height="12"
    viewBox="0 0 24 24"
    fill="none"
    stroke="rgba(255,255,255,0.5)"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <line x1="18" x2="6" y1="6" y2="18" />
    <line x1="6" x2="18" y1="6" y2="18" />
  </svg>
);

const formatTime = (s: number) => {
  const min = Math.floor(s / 60);
  const sec = s % 60;
  return `${min}:${sec.toString().padStart(2, "0")}`;
};

const TimerDisplay: React.FC<{ startTime: number }> = ({ startTime }) => {
  const [display, setDisplay] = useState("0:00");
  const rafRef = useRef<number>(0);

  useEffect(() => {
    const tick = () => {
      const elapsed = Math.floor((Date.now() - startTime) / 1000);
      setDisplay(formatTime(elapsed));
      rafRef.current = requestAnimationFrame(tick);
    };
    rafRef.current = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(rafRef.current);
  }, [startTime]);

  return <div className="timer-text">{display}</div>;
};

const AudioBars: React.FC = () => {
  const barsRef = useRef<HTMLDivElement>(null);
  const smoothedRef = useRef<number[]>(Array(16).fill(0));

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    const setup = async () => {
      unlisten = await listen<number[]>("mic-level", (event) => {
        const newLevels = event.payload;
        const smoothed = smoothedRef.current.map((prev, i) => {
          const target = newLevels[i] || 0;
          return prev * 0.65 + target * 0.35;
        });
        smoothedRef.current = smoothed;

        if (barsRef.current) {
          const bars = barsRef.current.children;
          for (let i = 0; i < bars.length; i++) {
            const v = smoothed[i] || 0;
            const el = bars[i] as HTMLElement;
            el.style.height = `${Math.min(20, 3 + Math.pow(v, 0.6) * 17)}px`;
            el.style.opacity = `${Math.max(0.25, v * 1.4)}`;
          }
        }
      });
    };
    setup();
    return () => {
      unlisten?.();
    };
  }, []);

  return (
    <div className="bars-container" ref={barsRef}>
      {Array.from({ length: 9 }, (_, i) => (
        <div key={i} className="bar" />
      ))}
    </div>
  );
};

const RecordingOverlay: React.FC = () => {
  const { t } = useTranslation();
  const [isVisible, setIsVisible] = useState(false);
  const [state, setState] = useState<OverlayState>("recording");
  const [timerStart, setTimerStart] = useState(0);
  const direction = getLanguageDirection(i18n.language);

  const handleCancel = useCallback(() => {
    commands.cancelOperation();
  }, []);

  useEffect(() => {
    let isMounted = true;
    let cleanupListeners: (() => void) | undefined;

    const setupEventListeners = async () => {
      const unlistenShow = await listen("show-overlay", async (event) => {
        await syncLanguageFromSettings();
        const overlayState = event.payload as OverlayState;
        setState(overlayState);
        setIsVisible(true);
        if (overlayState === "recording") {
          setTimerStart(Date.now());
        }
      });

      const unlistenHide = await listen("hide-overlay", () => {
        setIsVisible(false);
      });

      if (!isMounted) {
        unlistenShow();
        unlistenHide();
        return;
      }

      cleanupListeners = () => {
        unlistenShow();
        unlistenHide();
      };
    };

    setupEventListeners();
    return () => {
      isMounted = false;
      cleanupListeners?.();
    };
  }, []);

  return (
    <div
      dir={direction}
      className={`recording-overlay state-${state} ${isVisible ? "is-visible" : "is-hidden"}`}
    >
      <div className="overlay-left">
        {state === "recording" ? <MicIcon /> : <DotsIcon />}
      </div>

      <div className="overlay-middle">
        {state === "recording" && (
          <>
            <TimerDisplay startTime={timerStart} />
            <AudioBars />
          </>
        )}
        {state === "transcribing" && (
          <div className="transcribing-text">{t("overlay.transcribing")}</div>
        )}
        {state === "processing" && (
          <div className="transcribing-text">{t("overlay.processing")}</div>
        )}
      </div>

      <div className="overlay-right">
        {state === "recording" && (
          <div className="cancel-button" onClick={handleCancel}>
            <XIcon />
          </div>
        )}
      </div>
    </div>
  );
};

export default RecordingOverlay;
