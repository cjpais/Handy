import { listen } from "@tauri-apps/api/event";
import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  MicrophoneIcon,
  TranscriptionIcon,
  CancelIcon,
} from "../components/icons";
import "./RecordingOverlay.css";
import { commands, events } from "@/bindings";
import type {
  StreamPhase,
  StreamPhaseEvent,
  StreamTextEvent,
  StreamWorkKind,
} from "@/bindings";
import i18n, { syncLanguageFromSettings } from "@/i18n";
import { getLanguageDirection } from "@/lib/utils/rtl";

type OverlayState = "recording" | "streaming" | "transcribing" | "processing";

const SWAVE_BARS = 23;

const RecordingOverlay: React.FC = () => {
  const { t } = useTranslation();
  const [isVisible, setIsVisible] = useState(false);
  const [state, setState] = useState<OverlayState>("recording");
  const [levels, setLevels] = useState<number[]>(Array(16).fill(0));
  const [streamText, setStreamText] = useState<StreamTextEvent>({
    committed: "",
    tentative: "",
  });
  const [phase, setPhase] = useState<StreamPhase>("listening");
  const [workKind, setWorkKind] = useState<StreamWorkKind>("transcribing");
  const [elapsed, setElapsed] = useState(0);
  // Bumped on each new streaming session so the card remounts fresh (replays
  // the pop-in, and never animates in from the previous panel's open size).
  const [session, setSession] = useState(0);

  const smoothedLevelsRef = useRef<number[]>(Array(16).fill(0));
  const swaveRef = useRef<HTMLDivElement>(null);
  const swaveLevelsRef = useRef<number[]>(Array(SWAVE_BARS).fill(0));
  const lastWaveRef = useRef(0);
  const direction = getLanguageDirection(i18n.language);

  // Imperatively drive the streaming waveform from mic levels (avoids a React
  // re-render ~30×/sec). Heavily smoothed so it reads as a soft centered hump
  // rather than the peaky raw FFT: interpolate buckets → center "bell" weight →
  // spatial 1-2-1 blur → temporal smoothing with fast attack / slow decay.
  const updateSwave = (incoming: number[]) => {
    const el = swaveRef.current;
    if (!el || incoming.length === 0) return;
    // Throttle to ~20fps — calmer than the ~30/sec mic-level stream.
    const now = performance.now();
    if (now - lastWaveRef.current < 50) return;
    lastWaveRef.current = now;
    const bars = el.children;
    const n = SWAVE_BARS;

    // Overall loudness drives most of the shape (keeps it cohesive); a little
    // per-band detail keeps it alive.
    const overall = incoming.reduce((a, b) => a + b, 0) / incoming.length;

    const target = new Array(n);
    for (let i = 0; i < n; i++) {
      // interpolate the incoming buckets across n bars
      const p = (i / (n - 1)) * (incoming.length - 1);
      const lo = Math.floor(p);
      const hi = Math.min(incoming.length - 1, lo + 1);
      const band = incoming[lo] + (incoming[hi] - incoming[lo]) * (p - lo);
      const bell = Math.sin((i / (n - 1)) * Math.PI); // tall in the middle
      target[i] = bell * (overall * 0.6 + band * 0.4);
    }

    // A slow traveling ripple so silence reads as a gentle living wave rather
    // than a dead row of dots; it's dwarfed by real audio when you speak.
    const idlePhase = now / 700;
    for (let i = 0; i < n; i++) {
      // spatial smooth (1-2-1)
      const smoothed =
        (target[Math.max(0, i - 1)] +
          target[i] * 2 +
          target[Math.min(n - 1, i + 1)]) /
        4;
      // temporal smooth: rise quickly, fall slowly
      const prev = swaveLevelsRef.current[i];
      const a = smoothed > prev ? 0.3 : 0.1;
      const next = prev + (smoothed - prev) * a;
      swaveLevelsRef.current[i] = next;
      const idle = 1.8 * (0.5 + 0.5 * Math.sin(idlePhase + i * 0.55));
      const bar = bars[i] as HTMLElement | undefined;
      if (bar)
        bar.style.height = `${Math.max(2, Math.min(16, 2 + next * 28 + idle))}px`;
    }
  };

  useEffect(() => {
    const setupEventListeners = async () => {
      const unlistenShow = await listen("show-overlay", async (event) => {
        await syncLanguageFromSettings();
        const overlayState = event.payload as OverlayState;
        setState(overlayState);
        if (overlayState === "recording" || overlayState === "streaming") {
          setStreamText({ committed: "", tentative: "" });
        }
        if (overlayState === "streaming") {
          setPhase("listening");
          setWorkKind("transcribing");
          setElapsed(0);
          setSession((s) => s + 1); // remount the card fresh for this session
        }
        setIsVisible(true);
      });

      const unlistenHide = await listen("hide-overlay", () => {
        setIsVisible(false);
      });

      const unlistenLevel = await listen<number[]>("mic-level", (event) => {
        const newLevels = event.payload as number[];

        // Smoothed levels for the legacy pill bars.
        const smoothed = smoothedLevelsRef.current.map((prev, i) => {
          const target = newLevels[i] || 0;
          return prev * 0.7 + target * 0.3;
        });
        smoothedLevelsRef.current = smoothed;
        setLevels(smoothed.slice(0, 9));

        // Streaming waveform (imperative).
        updateSwave(newLevels);
      });

      const unlistenStream = await events.streamTextEvent.listen((event) => {
        setStreamText(event.payload);
      });

      const unlistenPhase = await events.streamPhaseEvent.listen((event) => {
        const payload: StreamPhaseEvent = event.payload;
        setPhase(payload.phase);
        if (payload.kind) setWorkKind(payload.kind);
      });

      return () => {
        unlistenShow();
        unlistenHide();
        unlistenLevel();
        unlistenStream();
        unlistenPhase();
      };
    };

    setupEventListeners();
  }, []);

  // Elapsed timer while the streaming overlay is visible.
  useEffect(() => {
    if (state !== "streaming" || !isVisible) return;
    const id = setInterval(() => setElapsed((e) => e + 1), 1000);
    return () => clearInterval(id);
  }, [state, isVisible]);

  const getIcon = () => {
    if (state === "recording" || state === "streaming") {
      return <MicrophoneIcon />;
    } else {
      return <TranscriptionIcon />;
    }
  };

  const fmtTime = (s: number) =>
    `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;

  // ---- Live preview card (pill → panel) ----
  if (state === "streaming") {
    const hasText =
      streamText.committed.length > 0 || streamText.tentative.length > 0;
    const working = phase === "working";
    const open = hasText && !working;

    const waveform = (
      <div className="swave" ref={swaveRef}>
        {Array.from({ length: SWAVE_BARS }, (_, i) => (
          <i key={i} />
        ))}
      </div>
    );
    const cancelBtn = (
      <button
        className="sx"
        aria-label="cancel"
        onClick={() => commands.cancelOperation()}
      >
        <svg viewBox="0 0 16 16" aria-hidden="true">
          <path
            d="M4 4 L12 12 M12 4 L4 12"
            stroke="currentColor"
            strokeWidth="1.6"
            strokeLinecap="round"
          />
        </svg>
      </button>
    );

    return (
      <div dir={direction} className="stream-stage">
        <div
          key={session}
          className={`scard ${open ? "open" : ""} ${working ? "working" : ""} ${
            isVisible ? "" : "leaving"
          }`}
        >
          <div className="stext">
            <div className="stext-clip">
              <div className="stext-cap">
                <p>
                  <span className="committed">
                    {streamText.committed ? streamText.committed + " " : ""}
                  </span>
                  <span className="tentative">{streamText.tentative}</span>
                  <span className="scaret" />
                </p>
              </div>
            </div>
          </div>
          {working ? (
            // working: spinner + label … cancel
            <div className="sbase sbase-working">
              <span className="sspinner" />
              <span className="swork-label">
                {workKind === "polishing"
                  ? t("overlay.processing")
                  : t("overlay.transcribing")}
              </span>
              {cancelBtn}
            </div>
          ) : (
            // dot (left) | waveform (center) | timer + cancel (right) — same
            // structure for pill & panel, so the morph is a pure width change.
            <div className="sbase">
              <div className="sbase-l">
                <span className="sdot" />
              </div>
              {waveform}
              <div className="sbase-r">
                {open && <span className="stimer">{fmtTime(elapsed)}</span>}
                {cancelBtn}
              </div>
            </div>
          )}
        </div>
      </div>
    );
  }

  // ---- Legacy compact pill (batch flow) ----
  return (
    <div
      dir={direction}
      className={`recording-overlay ${isVisible ? "fade-in" : ""}`}
    >
      <div className="overlay-left">{getIcon()}</div>

      <div className="overlay-middle">
        {state === "recording" && (
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
        {state === "transcribing" && (
          <div className="transcribing-text">{t("overlay.transcribing")}</div>
        )}
        {state === "processing" && (
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
