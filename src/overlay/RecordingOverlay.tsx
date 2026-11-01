import { listen } from "@tauri-apps/api/event";
import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import "./RecordingOverlay.css";
import { commands, events } from "@/bindings";
import type {
  OverlayEditSnapshot,
  StreamPhase,
  StreamPhaseEvent,
  StreamTextEvent,
  StreamWorkKind,
} from "@/bindings";
import i18n, { syncLanguageFromSettings } from "@/i18n";
import type { ModelStateEvent } from "@/lib/types/events";
import { getLanguageDirection } from "@/lib/utils/rtl";

type OverlayState =
  | "recording"
  | "streaming"
  | "transcribing"
  | "processing"
  | "editing";

// Number of reactive bars in the waveform (the simple, smoothed style shared by
// every overlay form). Mic levels arrive as 16 FFT buckets; we take the first N.
const WAVE_BARS = 9;

// Only call out a model load in the Live preview once it has run this long.
// Warm loads finish in well under this (~0.2s on Apple Silicon, ~1.5s on a
// Windows CPU backend), so the common case never flashes a loading notice while
// the user is speaking; cold loads can take 40s+.
const SLOW_MODEL_LOAD_MS = 2000;

const RecordingOverlay: React.FC = () => {
  const { t } = useTranslation();
  const [isVisible, setIsVisible] = useState(false);
  const [state, setState] = useState<OverlayState>("recording");
  // `Stream::play()` returning does not mean hardware callbacks are flowing.
  // Stay visually in an arming state until the backend processes the first
  // actual microphone sample chunk.
  const [captureReady, setCaptureReady] = useState(false);
  // Recording starts while the model loads in the background. Say so, otherwise
  // a cold start looks like an empty Live preview that ignores speech, then a
  // hung "Transcribing..." spinner. Once recording stops, any in-flight load is
  // the wait, so the working label reflects it immediately; the Live notice
  // waits for the load to be slow so fast loads don't flash it mid-speech.
  const [modelLoading, setModelLoading] = useState(false);
  const [modelLoadSlow, setModelLoadSlow] = useState(false);
  const modelLoadTimerRef = useRef<ReturnType<typeof setTimeout>>();
  // Latched per Live session: once the loading notice has opened the panel, keep
  // it open after the load so it doesn't collapse and reopen when text arrives.
  const [loadNoticeShown, setLoadNoticeShown] = useState(false);
  const [levels, setLevels] = useState<number[]>(Array(WAVE_BARS).fill(0));
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
  const [edit, setEdit] = useState<OverlayEditSnapshot | null>(null);
  const [draft, setDraft] = useState("");
  const [editError, setEditError] = useState<string | null>(null);
  const [editRequested, setEditRequested] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const editRef = useRef<OverlayEditSnapshot | null>(null);
  const editRequestedRef = useRef(false);
  const draftRef = useRef("");
  const draftRevisionRef = useRef(0);
  const baseAsrRef = useRef("");
  const editActivityRef = useRef(0);
  const editorRef = useRef<HTMLTextAreaElement>(null);
  const composingRef = useRef(false);
  const selectionRef = useRef<{
    start: number;
    end: number;
    direction: "forward" | "backward" | "none";
    scrollTop: number;
  } | null>(null);

  // Events, recovery and command responses can arrive out of order. Once a draft
  // exists, unacknowledged user edits win over incoming recognition. Server and
  // client revisions are separate: ASR can advance without acknowledging typing.
  const applyEditSnapshot = (next: OverlayEditSnapshot) => {
    const current = editRef.current;
    if (current && next.session_id < current.session_id) return;
    const sameSession = current?.session_id === next.session_id;
    if (sameSession && current && next.revision < current.revision) return;
    const preserveDraft =
      next.phase !== "inactive" &&
      sameSession &&
      (composingRef.current || next.user_revision < draftRevisionRef.current);
    editRef.current = next;
    setEdit(next);
    if (!preserveDraft) {
      const element = editorRef.current;
      if (
        element &&
        document.activeElement === element &&
        draftRef.current !== next.text
      ) {
        const oldText = draftRef.current;
        let prefix = 0;
        while (
          prefix < Math.min(oldText.length, next.text.length) &&
          oldText[prefix] === next.text[prefix]
        )
          prefix += 1;
        let suffix = 0;
        while (
          suffix < Math.min(oldText.length, next.text.length) - prefix &&
          oldText[oldText.length - suffix - 1] ===
            next.text[next.text.length - suffix - 1]
        )
          suffix += 1;
        const mapPosition = (offset: number) => {
          if (offset <= prefix) return offset;
          if (offset >= oldText.length - suffix)
            return offset + next.text.length - oldText.length;
          return Math.min(offset, next.text.length - suffix);
        };
        selectionRef.current = {
          start: mapPosition(element.selectionStart),
          end: mapPosition(element.selectionEnd),
          direction: element.selectionDirection,
          scrollTop: element.scrollTop,
        };
      }
      draftRef.current = next.text;
      draftRevisionRef.current = next.user_revision;
      baseAsrRef.current = next.asr_text;
      setDraft(next.text);
    }
    const active = next.phase !== "inactive";
    editRequestedRef.current = active;
    setEditRequested(active);
    if (active) {
      setState("editing");
      setIsVisible(true);
    }
  };

  const smoothedLevelsRef = useRef<number[]>(Array(16).fill(0));
  // Live-text scroll-back: the text region "sticks" to the newest line while the
  // user is at the bottom; if they scroll up to read history, auto-follow pauses
  // until they scroll back down.
  const capRef = useRef<HTMLDivElement>(null);
  const pinnedRef = useRef(true);
  const direction = getLanguageDirection(i18n.language);

  useEffect(() => {
    const setupEventListeners = async () => {
      const unlistenShow = await listen("show-overlay", async (event) => {
        const overlayState = event.payload as OverlayState;
        // Reset synchronously before settings I/O. A fast microphone can emit
        // recording-ready while the awaits below are in flight; resetting after
        // them would overwrite that event and leave the overlay stuck arming.
        if (overlayState === "recording" || overlayState === "streaming") {
          editActivityRef.current += 1;
          editRequestedRef.current = false;
          setEditRequested(false);
          setEditError(null);
          setSubmitting(false);
          setCaptureReady(false);
          smoothedLevelsRef.current = Array(16).fill(0);
          setLevels(Array(WAVE_BARS).fill(0));
          setStreamText({ committed: "", tentative: "" });
        }

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
        if (!editRequestedRef.current || overlayState === "editing")
          setState(overlayState);
        if (overlayState === "streaming") {
          setPhase("listening");
          setWorkKind("transcribing");
          setElapsed(0);
          setLoadNoticeShown(false);
          setSession((s) => s + 1); // remount the card fresh for this session
        }
        setIsVisible(true);
      });

      const unlistenHide = await listen("hide-overlay", () => {
        editActivityRef.current += 1;
        editRequestedRef.current = false;
        setEditRequested(false);
        setSubmitting(false);
        setIsVisible(false);
        setCaptureReady(false);
      });

      const unlistenReady = await listen("recording-ready", () => {
        setElapsed(0);
        setCaptureReady(true);
      });

      const unlistenLevel = await listen<number[]>("mic-level", (event) => {
        const newLevels = event.payload as number[];
        // Exponential smoothing across the 16 buckets, then take the first N
        // bars for the shared waveform.
        const smoothed = smoothedLevelsRef.current.map((prev, i) => {
          const target = newLevels[i] || 0;
          return prev * 0.7 + target * 0.3;
        });
        smoothedLevelsRef.current = smoothed;
        setLevels(smoothed.slice(0, WAVE_BARS));
      });

      const unlistenStream = await events.streamTextEvent.listen((event) => {
        if (!editRequestedRef.current) setStreamText(event.payload);
      });

      const unlistenEdit = await listen<OverlayEditSnapshot>(
        "overlay-edit-state",
        (event) => {
          editActivityRef.current += 1;
          applyEditSnapshot(event.payload);
        },
      );

      const unlistenPhase = await events.streamPhaseEvent.listen((event) => {
        const payload: StreamPhaseEvent = event.payload;
        setPhase(payload.phase);
        if (payload.kind) setWorkKind(payload.kind);
      });

      // The backend ends every `loading_started` with exactly one of completed
      // or failed, so only those end a load. Other events (e.g. `unloaded` from
      // the idle watcher) aren't ordered against an in-flight load and must not
      // clear it.
      const unlistenModel = await listen<ModelStateEvent>(
        "model-state-changed",
        (event) => {
          const type = event.payload.event_type;
          if (type === "loading_started") {
            clearTimeout(modelLoadTimerRef.current);
            setModelLoading(true);
            setModelLoadSlow(false);
            modelLoadTimerRef.current = setTimeout(
              () => setModelLoadSlow(true),
              SLOW_MODEL_LOAD_MS,
            );
          } else if (
            type === "loading_completed" ||
            type === "loading_failed"
          ) {
            clearTimeout(modelLoadTimerRef.current);
            setModelLoading(false);
            setModelLoadSlow(false);
          }
        },
      );

      // Install all listeners first so a ready event cannot fall into a gap.
      const recoveryActivity = editActivityRef.current;
      try {
        const result = await commands.getOverlayEditState();
        if (
          result.status === "ok" &&
          recoveryActivity === editActivityRef.current
        ) {
          applyEditSnapshot(result.data);
        }
      } catch {
        // The live overlay still works if recovery is temporarily unavailable.
      }

      return () => {
        unlistenShow();
        unlistenHide();
        unlistenReady();
        unlistenLevel();
        unlistenStream();
        unlistenEdit();
        unlistenPhase();
        unlistenModel();
        clearTimeout(modelLoadTimerRef.current);
      };
    };

    let disposed = false;
    let cleanup: (() => void) | undefined;
    void setupEventListeners().then((unlisten) => {
      if (disposed) unlisten();
      else cleanup = unlisten;
    });
    return () => {
      disposed = true;
      cleanup?.();
    };
  }, []);

  // Elapsed capture timer starts only once microphone samples are flowing.
  useEffect(() => {
    const recording = editRequested
      ? edit?.phase === "recording"
      : state === "streaming";
    if (!recording || !isVisible || !captureReady) return;
    const id = setInterval(() => setElapsed((e) => e + 1), 1000);
    return () => clearInterval(id);
  }, [state, isVisible, captureReady, editRequested, edit?.phase]);

  // Stick to the bottom as text streams in — but only while pinned, so a user who
  // has scrolled up to read history isn't yanked back down by the next chunk.
  useLayoutEffect(() => {
    if (editRequested) return;
    const el = capRef.current;
    if (!el) return;
    // Fade the top edge only once text actually overflows the cap.
    setOverflowing(el.scrollHeight > el.clientHeight + 1);
    if (pinnedRef.current) el.scrollTop = el.scrollHeight;
  }, [streamText, editRequested]);

  useEffect(() => {
    if (
      (edit?.phase === "recording" || edit?.phase === "review") &&
      editRequested &&
      !submitting
    ) {
      editorRef.current?.focus();
    }
  }, [edit?.phase, editRequested, submitting]);

  useLayoutEffect(() => {
    const selection = selectionRef.current;
    const element = editorRef.current;
    if (selection && element) {
      element.setSelectionRange(
        selection.start,
        selection.end,
        selection.direction,
      );
      element.scrollTop = selection.scrollTop;
      selectionRef.current = null;
    }
  }, [draft]);

  // `session` re-runs this when a new Live session starts mid-load (e.g. a
  // retry during a cold load), since the reset doesn't change the other deps.
  useEffect(() => {
    if (modelLoadSlow && state === "streaming") setLoadNoticeShown(true);
  }, [modelLoadSlow, state, session]);

  // Each fresh streaming session starts pinned to the bottom, fade cleared.
  useEffect(() => {
    pinnedRef.current = true;
    setOverflowing(false);
  }, [session]);

  if (!isVisible) return null;

  // Re-pin when the user is within ~a line of the bottom; unpin otherwise.
  const handleStreamScroll = () => {
    const el = capRef.current;
    if (!el) return;
    pinnedRef.current = el.scrollHeight - el.scrollTop - el.clientHeight <= 16;
  };

  const fmtTime = (s: number) =>
    `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;

  const cancelEdit = async () => {
    try {
      await commands.cancelOperation();
    } catch {
      setEditError("overlay.editCancelError");
    }
  };

  const beginEdit = async () => {
    if (editRequestedRef.current || editRef.current?.supported === false)
      return;
    editActivityRef.current += 1;
    editRequestedRef.current = true;
    setEditRequested(true);
    setEditError(null);
    const preview = [streamText.committed, streamText.tentative]
      .filter(Boolean)
      .join(" ");
    draftRef.current = preview;
    setDraft(preview);
    try {
      const result = await commands.beginOverlayEdit();
      if (!editRequestedRef.current) return;
      if (result.status === "ok") applyEditSnapshot(result.data);
      else throw new Error(result.error);
    } catch {
      // A ready event may have won the race with a lost command response.
      if (
        editRef.current?.phase === "recording" ||
        editRef.current?.phase === "finalizing" ||
        editRef.current?.phase === "review" ||
        editRef.current?.phase === "pasting"
      )
        return;
      editRequestedRef.current = false;
      setEditRequested(false);
      setState("streaming");
      setEditError("overlay.editBeginError");
    }
  };

  const updateDraft = (text: string) => {
    const current = editRef.current;
    if (
      !current ||
      (current.phase !== "recording" && current.phase !== "review") ||
      current.paste_requested ||
      submitting
    )
      return;
    const revision = draftRevisionRef.current + 1;
    draftRevisionRef.current = revision;
    draftRef.current = text;
    selectionRef.current = null;
    setDraft(text);
    setEditError(null);
    void commands
      .updateOverlayEditDraft(
        current.session_id,
        revision,
        baseAsrRef.current,
        text,
      )
      .then((result) => {
        if (
          result.status === "error" &&
          editRef.current?.session_id === current.session_id &&
          draftRevisionRef.current === revision
        ) {
          setEditError("overlay.editSaveError");
        }
      })
      .catch(() => {
        if (
          editRef.current?.session_id === current.session_id &&
          draftRevisionRef.current === revision
        ) {
          setEditError("overlay.editSaveError");
        }
      });
  };

  const pasteDraft = async () => {
    const current = editRef.current;
    if (
      !current ||
      (current.phase !== "recording" && current.phase !== "review") ||
      current.paste_requested ||
      submitting
    )
      return;
    setSubmitting(true);
    setEditError(null);
    try {
      const result = await commands.submitOverlayEdit(
        current.session_id,
        draftRevisionRef.current,
        baseAsrRef.current,
        draftRef.current,
      );
      if (result.status === "error") throw new Error(result.error);
    } catch {
      setEditError("overlay.editPasteError");
    } finally {
      setSubmitting(false);
    }
  };

  const transcribingLabel = modelLoading
    ? t("overlay.loadingModel")
    : t("overlay.transcribing");

  // ---- Shared building blocks (one visual language for every overlay form) ----
  const waveform = (
    <div className={`swave ${captureReady ? "ready" : "arming"}`}>
      {levels.map((v, i) => (
        <i
          key={i}
          style={{
            height: `${Math.max(3, Math.min(18, 3 + Math.pow(v, 0.7) * 15))}px`,
          }}
        />
      ))}
    </div>
  );

  const cancelBtn = (
    <button
      className="sx"
      aria-label={t("overlay.editCancel")}
      onClick={() => void cancelEdit()}
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

  if (editRequested || state === "editing") {
    const recording = edit?.phase === "recording";
    const ready =
      (recording || edit?.phase === "review") &&
      !edit?.paste_requested &&
      !submitting;
    return (
      <div dir={direction} className={`ov-stage ${position}`}>
        <div
          className="scard editor"
          onKeyDown={(event) => {
            if (event.nativeEvent.isComposing) return;
            if (event.key === "Escape") {
              event.preventDefault();
              void cancelEdit();
            } else if (event.key === "Enter" && event.ctrlKey) {
              event.preventDefault();
              void pasteDraft();
            }
          }}
        >
          <div className="sedit-heading" aria-live="polite">
            {recording ? (
              <span className={`sdot ${captureReady ? "ready" : "arming"}`} />
            ) : (
              !ready && <span className="sspinner" />
            )}
            <span>
              {t(
                edit?.phase === "pasting" || (submitting && !recording)
                  ? "overlay.editPasting"
                  : edit?.phase === "finalizing" ||
                      (recording && (submitting || edit?.paste_requested))
                    ? "overlay.editFinalizing"
                    : recording
                      ? "overlay.editRecording"
                      : ready
                        ? "overlay.editTranscript"
                        : "overlay.editPreparing",
              )}
            </span>
            {recording && (
              <span className="sedit-timer">{fmtTime(elapsed)}</span>
            )}
          </div>
          <textarea
            ref={editorRef}
            className="sedit-input"
            aria-label={t("overlay.editTranscript")}
            value={draft}
            readOnly={!ready}
            spellCheck
            onCompositionStart={() => {
              composingRef.current = true;
            }}
            onCompositionEnd={() => {
              composingRef.current = false;
            }}
            onChange={(event) => updateDraft(event.target.value)}
          />
          {edit?.conflict && (
            <div className="sedit-conflict">
              <p role="status">{t("overlay.editConflict")}</p>
              <label>
                <span>{t("overlay.editLatestRecognition")}</span>
                <textarea
                  className="sedit-recognition"
                  readOnly
                  value={edit.asr_text}
                  aria-label={t("overlay.editLatestRecognition")}
                />
              </label>
            </div>
          )}
          {editError && (
            <p className="sedit-error" role="alert">
              {t(editError)}
            </p>
          )}
          <div className="sedit-actions">
            <button type="button" onClick={() => void cancelEdit()}>
              {t("overlay.editCancel")}
            </button>
            <button
              type="button"
              className="sedit-paste"
              disabled={!ready}
              onClick={() => void pasteDraft()}
              title={t(
                recording
                  ? "overlay.editStopReviewShortcut"
                  : "overlay.editPasteShortcut",
              )}
            >
              {t(recording ? "overlay.editStopReview" : "overlay.editPaste")}
            </button>
          </div>
        </div>
      </div>
    );
  }

  // dot (left) | waveform (center) | timer + cancel (right) — same structure for
  // pill & panel, so the Live morph is a pure width change.
  const listeningRow = (showTimer: boolean, showCancel: boolean) => (
    <div className="sbase">
      <div className="sbase-l">
        <span className={`sdot ${captureReady ? "ready" : "arming"}`} />
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
    // While listening, a slow model load opens the panel with a notice where the
    // text would be, so speech that isn't transcribing yet doesn't look ignored.
    const loadingNotice = modelLoadSlow && !working && !hasText;
    // Keep the panel open whenever there's text — even while finalizing — so the
    // transcript stays put under a working spinner instead of collapsing and
    // squishing the text mid-stream. Only fall back to the small working pill
    // when there was no text to preserve.
    const open = hasText || loadingNotice || (loadNoticeShown && !working);
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
                className={`stext-cap stext-editable ${overflowing ? "overflowing" : ""}`}
                ref={capRef}
                onScroll={handleStreamScroll}
                role="button"
                tabIndex={0}
                aria-label={t("overlay.editLiveTranscript")}
                aria-disabled={edit?.supported === false}
                title={t(
                  edit?.supported === false
                    ? "overlay.editUnsupported"
                    : "overlay.editLiveTranscript",
                )}
                onClick={() => void beginEdit()}
                onKeyDown={(event) => {
                  if (
                    (event.key === "Enter" || event.key === " ") &&
                    !event.nativeEvent.isComposing
                  ) {
                    event.preventDefault();
                    void beginEdit();
                  }
                }}
              >
                <p>
                  {loadingNotice && (
                    <span className="sloading">
                      {t("overlay.loadingModel")}
                    </span>
                  )}
                  <span className="committed">
                    {streamText.committed ? streamText.committed + " " : ""}
                  </span>
                  <span className="tentative">{streamText.tentative}</span>
                  {/* Drop the blinking caret once finalizing — it's no longer
                      capturing, and a static spinner conveys the work. */}
                  {!working && !loadingNotice && <span className="scaret" />}
                </p>
                {editError && (
                  <p className="sedit-error" role="alert">
                    {t(editError)}
                  </p>
                )}
              </div>
            </div>
          </div>
          {working
            ? workingRow(
                workKind === "polishing"
                  ? t("overlay.processing")
                  : transcribingLabel,
                true,
              )
            : listeningRow(open, true)}
        </div>
      </div>
    );
  }

  // ---- Minimal overlay: exactly one row at a time — waveform (recording), or a
  // spinner + label (transcribing / processing). Never both. The pill animates its
  // width between them; the cancel button is in both rows so it stays put.
  const working = state === "transcribing" || state === "processing";
  const workLabel =
    state === "processing" ? t("overlay.processing") : transcribingLabel;

  return (
    <div
      dir={direction}
      className={`ov-stage ${position} ov-fade ${isVisible ? "show" : ""}`}
    >
      <div
        className={`scard compact ${working && isVisible ? "cworking" : ""}`}
      >
        {working ? workingRow(workLabel, true) : listeningRow(false, true)}
      </div>
    </div>
  );
};

export default RecordingOverlay;
