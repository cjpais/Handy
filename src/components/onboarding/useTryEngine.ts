/**
 * Try it's engine: Handy's recording events turned into where the person is
 * (which way, whether the note has the cursor, listening or typing) and what
 * the last take did. Either way counts for itself, and one is enough to
 * finish; a way done hands over to the other.
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
  /** A way done, the way it was asked for. */
  | { kind: "done"; way: Mode }
  /** The other way worked: it counts for that way. */
  | { kind: "credited"; way: Mode }
  | { kind: "nothingTyped" }
  | { kind: "empty" }
  | { kind: "failed" }
  | { kind: "slow"; wait: number };

/** A way done: the cheer stays this long before the next way starts. */
const ADVANCE_MS = 2600;

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

  // The ways this shortcut works: both, unless it's set to only one.
  const activation = getSetting("shortcut_activation");
  const ways: Mode[] =
    activation === "toggle"
      ? ["tap"]
      : activation === "push_to_talk"
        ? ["hold"]
        : ["hold", "tap"];

  const [lesson, setLesson] = useState<Mode>(
    () => ways.find((w) => !lessons[w]) ?? ways[0],
  );
  // The note has had the cursor (this way), and has it now.
  const [clicked, setClicked] = useState(false);
  const [focused, setFocused] = useState(false);
  /** The way of the take in progress, once it's known. */
  const [take, setTake] = useState<Mode | null>(null);
  const [phase, setPhase] = useState<Phase>("idle");
  // The keys on screen: down by this window's key events, or by Handy's own
  // when the global shortcut keeps them from reaching the window.
  const [pressDown, setPressDown] = useState(false);
  const [recordDown, setRecordDown] = useState(false);
  const [result, setResult] = useState<TakeResult>({ kind: "none" });
  const [fives, setFives] = useState(0);

  const field = useRef<HTMLTextAreaElement>(null);
  const lessonRef = useRef(lesson);
  lessonRef.current = lesson;
  const waysRef = useRef(ways);
  waysRef.current = ways;
  const takeRef = useRef<Mode | null>(null);
  const recordingRef = useRef(false);
  const lockedRef = useRef(false);
  const holdTimer = useRef<ReturnType<typeof setTimeout>>();
  const advanceTimer = useRef<ReturnType<typeof setTimeout>>();
  // The way the cheer is about to hand over to, if it hasn't yet.
  const pendingNext = useRef<Mode | null>(null);

  /** On to a way. Not mid-take: the words would go nowhere. */
  const goTo = (next: Mode) => {
    clearTimeout(advanceTimer.current);
    pendingNext.current = null;
    if (recordingRef.current || next === lessonRef.current) return;
    lessonRef.current = next;
    setLesson(next);
    setClicked(document.activeElement === field.current);
    setResult({ kind: "none" });
  };
  const goToRef = useRef(goTo);
  goToRef.current = goTo;

  const setWay = (way: Mode | null) => {
    takeRef.current = way;
    setTake(way);
  };

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
    const offState = events.recordingStateEvent.listen(({ payload }) => {
      const p = payload.phase;
      if (p === "started") {
        // Pressing again mid-cheer: on to the next way now, rather than
        // staying on the one just done.
        if (pendingNext.current) goToRef.current(pendingNext.current);
        recordingRef.current = true;
        lockedRef.current = false;
        setRecordDown(true);
        setPhase("listening");
        setResult({ kind: "none" });
        clearTimeout(holdTimer.current);
        // Still going at the threshold without locking on: they're holding.
        holdTimer.current = setTimeout(() => {
          if (!lockedRef.current) setWay("hold");
        }, threshold);
      } else if (p === "locked") {
        lockedRef.current = true;
        clearTimeout(holdTimer.current);
        setRecordDown(false);
        setWay("tap");
      } else {
        clearTimeout(holdTimer.current);
        setRecordDown(false);
        if (p === "cancelled") {
          recordingRef.current = false;
          setPhase("idle");
          setWay(null);
        } else {
          setPhase("typing");
          if (!takeRef.current) setWay(lockedRef.current ? "tap" : "hold");
        }
      }
    });
    const offDone = events.transcriptionCompleteEvent.listen(({ payload }) => {
      recordingRef.current = false;
      setPhase("idle");
      const way = takeRef.current ?? "hold";
      const now = lessonRef.current;
      setWay(null);
      if (payload.outcome !== "typed") {
        setResult({ kind: payload.outcome });
        return;
      }
      // The words go wherever the cursor is: only into the note if it's
      // there, and Handy is the app in front.
      if (document.activeElement !== field.current || !document.hasFocus()) {
        setResult({ kind: "nothingTyped" });
        return;
      }
      setLessons({ [way]: true });
      setFives((n) => n + 1);
      if (isTooSlow(payload.audio_seconds, payload.wait_seconds)) {
        setResult({ kind: "slow", wait: payload.wait_seconds });
        return;
      }
      if (way !== now) {
        setResult({ kind: "credited", way });
        return;
      }
      setResult({ kind: "done", way });
      const { lessons: done } = useOnboardingStore.getState();
      const next = waysRef.current.find((w) => w !== now && !done[w]);
      if (next) {
        pendingNext.current = next;
        advanceTimer.current = setTimeout(
          () => goToRef.current(next),
          ADVANCE_MS,
        );
      }
    });
    return () => {
      clearTimeout(holdTimer.current);
      clearTimeout(advanceTimer.current);
      offState.then((off) => off());
      offDone.then((off) => off());
    };
    // The refs carry what changes.
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
    ways,
    lessons,
    lesson,
    clicked,
    focused,
    take,
    phase,
    keysDown: pressDown || recordDown,
    result,
    fives,
    /** A high five from a click on the glove. */
    slap: () => setFives((n) => n + 1),
    field,
    fieldProps: {
      onFocus: () => {
        setFocused(true);
        setClicked(true);
      },
      onBlur: () => setFocused(false),
    },
  };
};
