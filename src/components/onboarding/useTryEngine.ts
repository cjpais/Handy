/** Try it's state: recording events mapped to phase, hold vs tap, and each take's result. */
import { useEffect, useRef, useState } from "react";
import { commands, events } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { useShortcutCapture } from "@/hooks/useShortcutCapture";
import type { OSType } from "@/lib/utils/keyboard";
import { useOnboardingStore } from "./onboardingStore";
import { isTooSlow } from "@/lib/models/catalog";

type Mode = "hold" | "tap";
type Phase = "idle" | "listening" | "typing";
type TakeResult =
  | { kind: "none" }
  /** First time for this way; the other is still to do. */
  | { kind: "done"; way: Mode }
  /** Both ways done. */
  | { kind: "finale" }
  /** It transcribed, but the cursor wasn't in the note. */
  | { kind: "nothingTyped" }
  /** The microphone sent only digital silence. */
  | { kind: "silent" }
  /** Sound, but no words in it. */
  | { kind: "noWords" };

/** In the order they're asked for. */
const WAYS: Mode[] = ["hold", "tap"];

const ADVANCE_MS = 2000;
const NO_WORDS_MS = 5000;

/**
 * Slack past the hold threshold before calling it a hold: the coordinator's
 * 50ms release grace means a tap just under the threshold reports "locked" late.
 */
const HOLD_MARGIN_MS = 150;

/** Typing longer than this shows "Watch the note." */
const WAIT_AFTER_MS = 400;
/** Minimum time the wait message stays up, so it never just flashes. */
const WAIT_SHOWN_MS = 500;

export const useTryEngine = ({ os }: { os: OSType }) => {
  const { getSetting } = useSettings();
  const { preview, lessons, setLessons } = useOnboardingStore();
  const shortcut = getSetting("bindings")?.transcribe?.current_binding ?? "";
  const threshold = getSetting("hold_threshold_ms") ?? 300;

  const todo = WAYS.find((w) => !lessons[w]);

  /** The current take's way, once known. */
  const [take, setTake] = useState<Mode | null>(null);
  const [phase, setPhase] = useState<Phase>("idle");
  const [recordDown, setRecordDown] = useState(false);
  const [result, setResult] = useState<TakeResult>({ kind: "none" });
  const [fives, setFives] = useState(0);
  /** Wait of the last take that was too slow for this model. */
  const [slowWait, setSlowWait] = useState<number | null>(null);
  const [waiting, setWaiting] = useState(false);

  const field = useRef<HTMLTextAreaElement>(null);
  const takeRef = useRef<Mode | null>(null);
  const lockedRef = useRef(false);
  const holdTimer = useRef<ReturnType<typeof setTimeout>>();
  const resultTimer = useRef<ReturnType<typeof setTimeout>>();
  const waitTimer = useRef<ReturnType<typeof setTimeout>>();
  const waitingSince = useRef<number | null>(null);
  /** A take's end, deferred until the wait message has had WAIT_SHOWN_MS. */
  const settleTimer = useRef<ReturnType<typeof setTimeout>>();
  const pendingSettle = useRef<(() => void) | null>(null);

  const setWay = (way: Mode | null) => {
    takeRef.current = way;
    setTake(way);
  };

  // Focus the note so the first take types there; refocusing it (or the
  // window, with the note focused) clears "nothingTyped".
  useEffect(() => {
    const note = field.current;
    note?.focus({ preventScroll: true });
    const fixed = () => {
      if (document.activeElement !== note) return;
      setResult((r) => (r.kind === "nothingTyped" ? { kind: "none" } : r));
    };
    note?.addEventListener("focus", fixed);
    window.addEventListener("focus", fixed);
    return () => {
      note?.removeEventListener("focus", fixed);
      window.removeEventListener("focus", fixed);
    };
  }, []);

  /* ---------- Handy's input ---------- */

  // Normally done on Permissions; repeated in case that screen was skipped.
  useEffect(() => {
    if (preview) return;
    commands.initializeEnigo().catch(console.error);
    commands.initializeShortcuts().catch(console.error);
  }, [preview]);

  /* ---------- takes ---------- */

  useEffect(() => {
    const stopWaiting = () => {
      clearTimeout(waitTimer.current);
      waitingSince.current = null;
      setWaiting(false);
    };
    const flushSettle = () => {
      clearTimeout(settleTimer.current);
      const settle = pendingSettle.current;
      pendingSettle.current = null;
      settle?.();
    };

    const offState = events.recordingStateEvent.listen(({ payload }) => {
      const p = payload.phase;
      if (p === "started") {
        flushSettle();
        // The last result stays up until this take's way is known.
        clearTimeout(resultTimer.current);
        lockedRef.current = false;
        setRecordDown(true);
        setPhase("listening");
        clearTimeout(holdTimer.current);
        // Past the threshold without locking: a hold.
        holdTimer.current = setTimeout(() => {
          if (!lockedRef.current) setWay("hold");
        }, threshold + HOLD_MARGIN_MS);
      } else if (p === "locked") {
        lockedRef.current = true;
        clearTimeout(holdTimer.current);
        setRecordDown(false);
        setWay("tap");
      } else {
        clearTimeout(holdTimer.current);
        setRecordDown(false);
        if (p === "cancelled") {
          stopWaiting();
          setPhase("idle");
          setWay(null);
          setResult({ kind: "none" });
        } else {
          // An unknown way is settled when the words land.
          setPhase("typing");
          clearTimeout(waitTimer.current);
          waitTimer.current = setTimeout(() => {
            waitingSince.current = performance.now();
            setWaiting(true);
          }, WAIT_AFTER_MS);
        }
      }
    });
    const offDone = events.transcriptionCompleteEvent.listen(({ payload }) => {
      clearTimeout(waitTimer.current);
      const way = takeRef.current ?? (lockedRef.current ? "tap" : "hold");
      // Read focus now, not when the result is shown.
      const landed =
        document.activeElement === field.current && document.hasFocus();
      const settle = () => {
        waitingSince.current = null;
        setWaiting(false);
        setPhase("idle");
        setWay(null);
        // Failures already get the app's error toast.
        if (payload.outcome === "failed") {
          setResult({ kind: "none" });
          return;
        }
        if (payload.outcome === "silent") {
          setResult({ kind: "silent" });
          return;
        }
        // Nothing to fix, so this one times out.
        if (payload.outcome === "empty") {
          setResult({ kind: "noWords" });
          clearTimeout(resultTimer.current);
          resultTimer.current = setTimeout(
            () =>
              setResult((r) => (r.kind === "noWords" ? { kind: "none" } : r)),
            NO_WORDS_MS,
          );
          return;
        }
        if (!landed) {
          setResult({ kind: "nothingTyped" });
          return;
        }
        // Read fresh: this listener outlives the render it was made in.
        const before = useOnboardingStore.getState().lessons;
        const fresh = !before[way];
        if (fresh) setLessons({ [way]: true });
        // Slow still counts; Try it suggests a faster model.
        if (isTooSlow(payload.audio_seconds, payload.wait_seconds))
          setSlowWait(payload.wait_seconds);
        if (!fresh) {
          setResult({ kind: "none" });
          return;
        }
        // The finale does its own high five, so no fives++ here.
        if (!WAYS.some((w) => w !== way && !before[w])) {
          setResult({ kind: "finale" });
          return;
        }
        setFives((n) => n + 1);
        setResult({ kind: "done", way });
        resultTimer.current = setTimeout(
          () => setResult({ kind: "none" }),
          ADVANCE_MS,
        );
      };
      // If the wait message is up, keep it for WAIT_SHOWN_MS before settling.
      const shown =
        waitingSince.current == null
          ? Infinity
          : performance.now() - waitingSince.current;
      if (shown >= WAIT_SHOWN_MS) {
        settle();
        return;
      }
      pendingSettle.current = settle;
      settleTimer.current = setTimeout(flushSettle, WAIT_SHOWN_MS - shown);
    });
    return () => {
      clearTimeout(holdTimer.current);
      clearTimeout(resultTimer.current);
      clearTimeout(waitTimer.current);
      clearTimeout(settleTimer.current);
      offState.then((off) => off());
      offDone.then((off) => off());
    };
  }, [setLessons, threshold]);

  // Refocus the note after a shortcut change so the next take lands there.
  const capture = useShortcutCapture("transcribe", os, {
    onDone: () => field.current?.focus({ preventScroll: true }),
    inert: preview,
    escapeCancels: true,
  });

  return {
    shortcut,
    lessons,
    todo,
    take,
    phase,
    waiting,
    keysDown: recordDown,
    result,
    dismiss: () => setResult({ kind: "none" }),
    slowWait,
    fives,
    capture,
    slap: () => setFives((n) => n + 1),
    field,
  };
};
