import { listen } from "@tauri-apps/api/event";
import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
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

type OverlayState =
  | "recording"
  | "streaming"
  | "transcribing"
  | "processing"
  | "error";

// Number of reactive bars in the canvas waveform — one per FFT band from the
// backend (BUCKETS in recorder.rs). Each bar's top and bottom animate
// independently, so the shape reads as a lively, non-mirrored waveform.
const WAVE_BARS = 28;

const RecordingOverlay: React.FC = () => {
  const { t } = useTranslation();
  const [isVisible, setIsVisible] = useState(false);
  const [state, setState] = useState<OverlayState>("recording");
  const canvasRef = useRef<HTMLCanvasElement>(null);
  // Holds the canvas draw fn so the mic-level event handler can drive it directly.
  // The overlay is a non-activating NSPanel where WebKit throttles rAF to ~0, so
  // the animation must be pushed by the incoming level events, not pulled by rAF.
  const renderRef = useRef<() => void>(() => {});
  const [streamText, setStreamText] = useState<StreamTextEvent>({
    committed: "",
    tentative: "",
  });
  const [phase, setPhase] = useState<StreamPhase>("listening");
  const [workKind, setWorkKind] = useState<StreamWorkKind>("transcribing");
  const [elapsed, setElapsed] = useState(0);
  // Bumped on each new streaming session so the Live card remounts fresh (replays
  // the pop-in, and never animates in from the previous panel's open size).
  const [session, setSession] = useState(0);
  // Overlay placement (top vs bottom of the screen). The Live panel grows downward
  // from a top overlay (oldest line under the pill) and upward from a bottom one.
  const [position, setPosition] = useState<"top" | "bottom">("bottom");
  // True once live text overflows the cap. A top overlay fades its top edge only
  // while overflowing, so the resting first line stays crisp flush under the pill.
  const [overflowing, setOverflowing] = useState(false);

  // Latest raw band levels from the backend; the canvas rAF loop reads this and
  // does all smoothing/animation, so mic packets never trigger React re-renders.
  const latestLevelsRef = useRef<number[]>(new Array(WAVE_BARS).fill(0));
  // Live-text scroll-back: the text region "sticks" to the newest line while the
  // user is at the bottom; if they scroll up to read history, auto-follow pauses
  // until they scroll back down.
  const capRef = useRef<HTMLDivElement>(null);
  const pinnedRef = useRef(true);
  const direction = getLanguageDirection(i18n.language);

  useEffect(() => {
    const setupEventListeners = async () => {
      const unlistenShow = await listen("show-overlay", async (event) => {
        await syncLanguageFromSettings();
        // The Live panel flows downward from a top overlay and upward from a
        // bottom one; read the placement so the layout can flip to match.
        try {
          const settings = await commands.getAppSettings();
          if (settings.status === "ok") {
            setPosition(
              settings.data.overlay_position === "top" ? "top" : "bottom",
            );
          }
        } catch {
          // Keep the previous/default placement if settings can't be read.
        }
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
        // Store raw band levels and draw a frame now. Event-driven so the waveform
        // animates even when WebKit has throttled rAF in this background panel.
        latestLevelsRef.current = event.payload as number[];
        renderRef.current();
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

  // Elapsed timer while the Live overlay is visible.
  useEffect(() => {
    if (state !== "streaming" || !isVisible) return;
    const id = setInterval(() => setElapsed((e) => e + 1), 1000);
    return () => clearInterval(id);
  }, [state, isVisible]);

  // Canvas waveform: 60fps render of asymmetric center bars. Each bar maps to one
  // FFT band; its top and bottom animate independently (fast attack, slow release)
  // so the shape reads as a lively, non-mirrored waveform. Silence settles to a
  // thin flat line. Runs for the component's life; skips frames when the canvas
  // isn't mounted (working states show a spinner row instead of the waveform).
  useEffect(() => {
    const bars = Array.from({ length: WAVE_BARS }, () => ({
      top: 0,
      bot: 0,
      ttop: 0,
      tbot: 0,
      ntop: 0,
      nbot: 0,
    }));
    let last = performance.now();
    const render = () => {
      const now = performance.now();
      const dt = Math.min(0.05, Math.max(0.001, (now - last) / 1000));
      last = now;
      const cv = canvasRef.current;
      const ctx = cv?.getContext("2d");
      if (!cv || !ctx) return;

      const dpr = Math.max(1, window.devicePixelRatio || 1);
      const cssW = cv.clientWidth || 188;
      const cssH = cv.clientHeight || 32;
      if (cv.width !== Math.round(cssW * dpr)) cv.width = Math.round(cssW * dpr);
      if (cv.height !== Math.round(cssH * dpr))
        cv.height = Math.round(cssH * dpr);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, cssW, cssH);
      ctx.fillStyle = getComputedStyle(cv).color || "#f28cbb";

      // dt-based easing so the feel is the same whether we're ticking at the
      // ~24Hz event rate (rAF throttled) or 60fps rAF (panel focused).
      const kAtt = 1 - Math.exp(-38 * dt);
      const kRel = 1 - Math.exp(-13 * dt);
      const L = latestLevelsRef.current;
      const n = WAVE_BARS;
      const slot = cssW / n;
      const bw = Math.min(5, slot * 0.62);
      const r = bw / 2;
      const mid = cssH / 2;
      const maxAmp = cssH / 2 - 1;
      for (let i = 0; i < n; i++) {
        // Map the band even if the backend sent a different count than WAVE_BARS.
        const raw =
          L.length === n ? L[i] || 0 : L[Math.floor((i / n) * L.length)] || 0;
        // Center-weight the bars: a mild arch (center taller) plus a gentle treble
        // lift that offsets the bass-heavy speech spectrum, so the visual mass sits
        // in the middle instead of piling up on the low-frequency (left) side.
        const pos = n > 1 ? i / (n - 1) : 0.5;
        const weight =
          (0.6 + 0.4 * Math.sin(Math.PI * pos)) * (0.78 + 0.44 * pos);
        const base = raw * weight;
        const b = bars[i];
        b.ntop -= dt;
        b.nbot -= dt;
        // Re-roll top/bottom targets on independent staggered timers so the two
        // edges never move in lockstep (the "not mirrored" ask).
        if (b.ntop <= 0) {
          b.ttop = base * (0.38 + Math.random() * 0.92);
          b.ntop = 0.03 + Math.random() * 0.08;
        }
        if (b.nbot <= 0) {
          b.tbot = base * (0.38 + Math.random() * 0.92);
          b.nbot = 0.03 + Math.random() * 0.08;
        }
        b.top += (b.ttop - b.top) * (b.ttop > b.top ? kAtt : kRel);
        b.bot += (b.tbot - b.bot) * (b.tbot > b.bot ? kAtt : kRel);
        const up = Math.max(1.5, b.top * maxAmp);
        const dn = Math.max(1.5, b.bot * maxAmp);
        const x = i * slot + (slot - bw) / 2;
        ctx.beginPath();
        if (ctx.roundRect) ctx.roundRect(x, mid - up, bw, up + dn, r);
        else ctx.rect(x, mid - up, bw, up + dn);
        ctx.fill();
      }
    };
    // Push draws from mic-level events (see renderRef) so motion survives rAF
    // throttling; rAF adds extra smoothness only while the panel is focused.
    renderRef.current = render;
    let raf = 0;
    const loop = () => {
      render();
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, []);

  // Stick to the bottom as text streams in — but only while pinned, so a user who
  // has scrolled up to read history isn't yanked back down by the next chunk.
  useLayoutEffect(() => {
    const el = capRef.current;
    if (!el) return;
    // Fade the top edge only once text actually overflows the cap.
    setOverflowing(el.scrollHeight > el.clientHeight + 1);
    if (pinnedRef.current) el.scrollTop = el.scrollHeight;
  }, [streamText]);

  // Each fresh streaming session starts pinned to the bottom, fade cleared.
  useEffect(() => {
    pinnedRef.current = true;
    setOverflowing(false);
  }, [session]);

  // Re-pin when the user is within ~a line of the bottom; unpin otherwise.
  const handleStreamScroll = () => {
    const el = capRef.current;
    if (!el) return;
    pinnedRef.current = el.scrollHeight - el.scrollTop - el.clientHeight <= 16;
  };

  const fmtTime = (s: number) =>
    `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;

  // ---- Shared building blocks (one visual language for every overlay form) ----
  const waveform = <canvas ref={canvasRef} className="swave" aria-hidden="true" />;

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

  // dot (left) | waveform (center) | timer + cancel (right) — same structure for
  // pill & panel, so the Live morph is a pure width change.
  const listeningRow = (showTimer: boolean, showCancel: boolean) => (
    <div className="sbase">
      <div className="sbase-l">
        <span className="sdot" />
      </div>
      {waveform}
      <div className="sbase-r">
        {showTimer && <span className="stimer">{fmtTime(elapsed)}</span>}
        {showCancel && cancelBtn}
      </div>
    </div>
  );

  // spinner (left) | label (center) | cancel (right) — same 3-zone grid as the
  // listening row, so the label is centered.
  const workingRow = (label: string, showCancel: boolean) => (
    <div className="sbase">
      <div className="sbase-l">
        <span className="sspinner" />
      </div>
      <span className="swork-label">{label}</span>
      <div className="sbase-r">{showCancel && cancelBtn}</div>
    </div>
  );

  // ---- Live overlay: a pill that sculpts open into a panel ----
  if (state === "streaming") {
    const hasText =
      streamText.committed.length > 0 || streamText.tentative.length > 0;
    const working = phase === "working";
    // Keep the panel open whenever there's text — even while finalizing — so the
    // transcript stays put under a working spinner instead of collapsing and
    // squishing the text mid-stream. Only fall back to the small working pill
    // when there was no text to preserve.
    const open = hasText;
    const collapsed = working && !hasText;

    return (
      <div dir={direction} className={`ov-stage ${position}`}>
        <div
          key={session}
          className={`scard ${open ? "open" : ""} ${collapsed ? "working" : ""} ${
            isVisible ? "" : "leaving"
          }`}
        >
          <div className="stext">
            <div className="stext-clip">
              <div
                className={`stext-cap ${overflowing ? "overflowing" : ""}`}
                ref={capRef}
                onScroll={handleStreamScroll}
              >
                <p>
                  <span className="committed">
                    {streamText.committed ? streamText.committed + " " : ""}
                  </span>
                  <span className="tentative">{streamText.tentative}</span>
                  {/* Drop the blinking caret once finalizing — it's no longer
                      capturing, and a static spinner conveys the work. */}
                  {!working && <span className="scaret" />}
                </p>
              </div>
            </div>
          </div>
          {working
            ? workingRow(
                workKind === "polishing"
                  ? t("overlay.processing")
                  : t("overlay.transcribing"),
                true,
              )
            : listeningRow(open, true)}
        </div>
      </div>
    );
  }

  // ---- Minimal overlay: exactly one row at a time — waveform (recording), a
  // spinner + label (transcribing / processing), or an error label. Never
  // several. The pill animates its width between them; the cancel button is in
  // both active rows so it stays put.
  const working = state === "transcribing" || state === "processing";
  const errored = state === "error";
  const workLabel =
    state === "processing"
      ? t("overlay.processing")
      : t("overlay.transcribing");

  const errorRow = (
    <div className="sbase">
      <div className="sbase-l" />
      <span className="swork-label serr">{t("overlay.commandFailed")}</span>
      <div className="sbase-r" />
    </div>
  );

  return (
    <div
      dir={direction}
      className={`ov-stage ${position} ov-fade ${isVisible ? "show" : ""}`}
    >
      <div
        className={`scard compact ${(working || errored) && isVisible ? "cworking" : ""}`}
      >
        {errored
          ? errorRow
          : working
            ? workingRow(workLabel, true)
            : listeningRow(false, true)}
      </div>
    </div>
  );
};

export default RecordingOverlay;
