/**
 * Try it's engine: Handy's recording events turned into what the person is
 * doing (listening or typing, and whether it's a hold or a tap) and what the
 * last take did. Both ways count, in either order, and one is enough to
 * finish; the way still to do is hold first, then tap.
 */
import { useEffect, useRef, useState } from "react";
import { commands, events } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { useShortcutPress } from "@/hooks/useShortcutPress";
import type { OSType } from "@/lib/utils/keyboard";
import { useOnboardingStore } from "./onboardingStore";
import { useSuspendedShortcuts } from "./useSuspendedShortcuts";
import { isTooSlow } from "./data/models";

export type Mode = "hold" | "tap";
export type Phase = "idle" | "listening" | "typing";
export type TakeResult =
  | { kind: "none" }
  /** A way done for the first time, with the other still to do. */
  | { kind: "done"; way: Mode }
  | { kind: "nothingTyped" }
  | { kind: "empty" }
  | { kind: "failed" }
  /** It landed (and counts), but too slowly for this model here. */
  | { kind: "slow"; wait: number };

/** The ways, in the order they're asked for. */
const WAYS: Mode[] = ["hold", "tap"];

/** A way done: the cheer stays this long before the next way's line. */
const ADVANCE_MS = 2600;

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

export const useTryEngine = ({
  ready,
  os,
}: {
  /** The model is on disk: the shortcut records. */
  ready: boolean;
  os: OSType;
}) => {
  const { getSetting } = useSettings();
  const { preview, lessons, setLessons } = useOnboardingStore();
  const shortcut = getSetting("bindings")?.transcribe?.current_binding ?? "";
  const threshold = getSetting("hold_threshold_ms") ?? 300;

  /** The way still to do; none once both are done. */
  const todo = WAYS.find((w) => !lessons[w]);

  /** The way of the take in progress, once it's known. */
  const [take, setTake] = useState<Mode | null>(null);
  const [phase, setPhase] = useState<Phase>("idle");
  // The keys on screen: down by this window's key events, or by Handy's own
  // when the global shortcut keeps them from reaching the window.
  const [pressDown, setPressDown] = useState(false);
  const [recordDown, setRecordDown] = useState(false);
  const [result, setResult] = useState<TakeResult>({ kind: "none" });
  const [fives, setFives] = useState(0);
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
  useEffect(() => {
    field.current?.focus({ preventScroll: true });
  }, []);

  /* ---------- Handy's input ---------- */

  useEffect(() => {
    if (!preview) commands.initializeEnigo().catch(console.error);
  }, [preview]);

  // The shortcut records only once there's a model to transcribe with.
  useSuspendedShortcuts(!preview && !ready);
  useEffect(() => {
    if (preview || !ready) return;
    commands
      .initializeShortcuts()
      .then(() => commands.resumeAllBindings())
      .catch(console.error);
  }, [preview, ready]);

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
        if (payload.outcome !== "typed") {
          setResult({ kind: payload.outcome });
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
        // Slow still counts, but it's a problem to fix, not a high five.
        if (isTooSlow(payload.audio_seconds, payload.wait_seconds)) {
          setResult({ kind: "slow", wait: payload.wait_seconds });
          return;
        }
        // A way already done: the words landing say enough.
        if (!fresh) {
          setResult({ kind: "none" });
          return;
        }
        setFives((n) => n + 1);
        // The last way: the finale shows whenever nothing's left to do.
        if (!WAYS.some((w) => w !== way && !before[w])) {
          setResult({ kind: "none" });
          return;
        }
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

  // The keys on screen follow the keys under the fingers where the window
  // hears them: a tap that stops a take still shows as pressed.
  useShortcutPress(shortcut, os, threshold, {
    onDown: () => setPressDown(true),
    onUp: () => setPressDown(false),
  });

  return {
    preview,
    shortcut,
    lessons,
    todo,
    take,
    phase,
    waiting,
    keysDown: pressDown || recordDown,
    result,
    fives,
    /** A high five from a click on the glove. */
    slap: () => setFives((n) => n + 1),
    field,
  };
};
