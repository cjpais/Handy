/**
 * Try it's engine: Handy's recording events turned into what the person is
 * doing (listening or typing, and whether it's a hold or a tap) and what the
 * last take did. Both ways count, in either order, and one is enough to
 * finish; the way still to do is hold first, then tap.
 */
import { useEffect, useRef, useState } from "react";
import { commands, events } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { useShortcutCapture } from "@/hooks/useShortcutCapture";
import type { OSType } from "@/lib/utils/keyboard";
import { useOnboardingStore } from "./onboardingStore";
import { isTooSlow } from "./data/models";

type Mode = "hold" | "tap";
type Phase = "idle" | "listening" | "typing";
type TakeResult =
  | { kind: "none" }
  /** A way done for the first time, with the other still to do. */
  | { kind: "done"; way: Mode }
  /** The last way done: Try it's finale (the high five and All set) plays. */
  | { kind: "finale" }
  /** It transcribed, but the cursor wasn't in the note. */
  | { kind: "nothingTyped" }
  /** The microphone sent only digital silence. */
  | { kind: "silent" }
  /** Sound, but no words in it. */
  | { kind: "noWords" };

/** The ways, in the order they're asked for. */
const WAYS: Mode[] = ["hold", "tap"];

/** A way done: the cheer stays this long before the next way's line. */
const ADVANCE_MS = 2000;
/** "I didn't catch any words" stays this long, then back to what's next. */
const NO_WORDS_MS = 5000;

/**
 * Past the hold threshold, how long to wait before calling a take a hold. A
 * release is only resolved after the coordinator's 50ms release grace, so a
 * tap let go just under the threshold reports "locked" a little after it.
 */
const HOLD_MARGIN_MS = 150;

/**
 * After the keys come up, the words usually land within a blink. Only if
 * they haven't by this long is it a wait worth saying so ("Watch the note.");
 * until then the line they let go on stays, dimmed.
 */
const WAIT_AFTER_MS = 400;
/** Once the wait's been said, it stays this long, so it never just flashes. */
const WAIT_SHOWN_MS = 500;

export const useTryEngine = ({ os }: { os: OSType }) => {
  const { getSetting } = useSettings();
  const { preview, lessons, setLessons } = useOnboardingStore();
  const shortcut = getSetting("bindings")?.transcribe?.current_binding ?? "";
  const threshold = getSetting("hold_threshold_ms") ?? 300;

  /** The way still to do; none once both are done. */
  const todo = WAYS.find((w) => !lessons[w]);

  /** The way of the take in progress, once it's known. */
  const [take, setTake] = useState<Mode | null>(null);
  const [phase, setPhase] = useState<Phase>("idle");
  /** The keys on screen, down while Handy says it's recording. */
  const [recordDown, setRecordDown] = useState(false);
  const [result, setResult] = useState<TakeResult>({ kind: "none" });
  const [fives, setFives] = useState(0);
  /** The last take that landed too slowly for this model here: its wait. */
  const [slowWait, setSlowWait] = useState<number | null>(null);
  /** Typing has taken long enough to say so. */
  const [waiting, setWaiting] = useState(false);

  const field = useRef<HTMLTextAreaElement>(null);
  const takeRef = useRef<Mode | null>(null);
  const lockedRef = useRef(false);
  const holdTimer = useRef<ReturnType<typeof setTimeout>>();
  const resultTimer = useRef<ReturnType<typeof setTimeout>>();
  const waitTimer = useRef<ReturnType<typeof setTimeout>>();
  /** When the wait was said, while it's up. */
  const waitingSince = useRef<number | null>(null);
  /** A take's end, held back until the wait's been up long enough. */
  const settleTimer = useRef<ReturnType<typeof setTimeout>>();
  const pendingSettle = useRef<(() => void) | null>(null);

  const setWay = (way: Mode | null) => {
    takeRef.current = way;
    setTake(way);
  };

  // The note has the cursor from the start, so the first press types there.
  // The cursor back in it, with Handy in front, is the fix for "nothing
  // landed", so that goes: the note getting focus, or Handy coming back to
  // the front with the note still focused.
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

  useEffect(() => {
    if (!preview) commands.initializeEnigo().catch(console.error);
  }, [preview]);

  // Normally on since Permissions; this is for when that screen was skipped.
  // Without a model the backend ignores the shortcut until one is on disk.
  useEffect(() => {
    if (!preview) commands.initializeShortcuts().catch(console.error);
  }, [preview]);

  /* ---------- takes ---------- */

  useEffect(() => {
    const stopWaiting = () => {
      clearTimeout(waitTimer.current);
      waitingSince.current = null;
      setWaiting(false);
    };
    // A take's end that's still held back happens now, before anything new.
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
        // Whatever the last take said stays up until this one's way is
        // known (Try it shows it until then), so the cheer mustn't time out
        // underneath it.
        clearTimeout(resultTimer.current);
        lockedRef.current = false;
        setRecordDown(true);
        setPhase("listening");
        clearTimeout(holdTimer.current);
        // Still going past the threshold without locking on: they're holding.
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
          // The way stays unknown if it was: what was up stays, dimmed, and
          // the way is settled when the words land.
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
      // Where the words went is a fact of now, whenever it's shown.
      const landed =
        document.activeElement === field.current && document.hasFocus();
      const settle = () => {
        waitingSince.current = null;
        setWaiting(false);
        setPhase("idle");
        setWay(null);
        // A failure has the app's own error toast; the glove goes on with
        // what's next.
        if (payload.outcome === "failed") {
          setResult({ kind: "none" });
          return;
        }
        if (payload.outcome === "silent") {
          setResult({ kind: "silent" });
          return;
        }
        // No words: nothing to fix but trying again, so it doesn't stay.
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
        // The words go wherever the cursor is: only into the note if it's
        // there, and Handy is the app in front.
        if (!landed) {
          setResult({ kind: "nothingTyped" });
          return;
        }
        // Read fresh: this listener outlives the render it was made in.
        const before = useOnboardingStore.getState().lessons;
        const fresh = !before[way];
        if (fresh) setLessons({ [way]: true });
        // Slow still counts, high five and all; the screen suggests a
        // faster model under itself.
        if (isTooSlow(payload.audio_seconds, payload.wait_seconds))
          setSlowWait(payload.wait_seconds);
        // A way already done: the words landing say enough.
        if (!fresh) {
          setResult({ kind: "none" });
          return;
        }
        // The last way: the finale, whose high five comes after a wind-up,
        // so the glove doesn't slap here.
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
      // The wait, if it was said, gets its moment; otherwise straight on.
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

  // Changing the shortcut, from the keys in the line: the recorder Settings
  // and Demo use, which turns Handy's shortcuts off while it listens and back
  // on after. The cursor goes back to the note, so the next take lands there.
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
    /** Back to what's next, after a problem's been dealt with. */
    dismiss: () => setResult({ kind: "none" }),
    slowWait,
    fives,
    /** Recording a new shortcut: `capturing`, what's `partial`ly held, start, cancel. */
    capture,
    /** A high five from a click on the glove. */
    slap: () => setFives((n) => n + 1),
    field,
  };
};
