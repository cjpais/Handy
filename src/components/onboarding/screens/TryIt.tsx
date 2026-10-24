import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import {
  bindingKeys,
  isOnlyModifiers,
  keycapLabel,
  type OSType,
  spaceDoesNotClick,
} from "@/lib/utils/keyboard";
import { prefersReducedMotion } from "@/lib/utils/motion";
import { Button } from "../../ui/Button";
import { TextLink } from "../../ui/TextLink";
import { Keycaps, KeySlot } from "../../shared/Keycaps";
import { Mascot } from "../../shared/Mascot";
import TrayIcon from "../../icons/TrayIcon";
import { DesktopBar } from "../DesktopBar";
import { WindowBar } from "../WindowBar";
import { OnboardingShell, ScreenSub, ScreenTitle } from "../OnboardingShell";
import { LinuxGuide } from "../LinuxGuide";
import { TrayPanel } from "../TrayPanel";
import { useOnboardingStore } from "../onboardingStore";
import { useTryEngine } from "../useTryEngine";
import { MicrophoneDropdown } from "../../settings/MicrophoneSelector";
import { useSettings } from "@/hooks/useSettings";
import type { CatalogModel } from "../data/models";
import "./tryit.css";

type Mood = "point" | "listen" | "cheer" | "oops" | "ready" | "windup";

/**
 * Try it's end, the move into All set: the glove winds up (only when the last
 * take finishes it), high-fives (not for Skip), flies into Handy's icon in
 * the menu bar or tray, and the icon's panel opens.
 */
type Stage = "try" | "windup" | "five" | "fly" | "set";
/** What started it: the last take, the High five button (or Finish), Skip. */
type How = "auto" | "five" | "skip";

/** "Give me five" is up this long, the glove winding up, before it's given. */
const WINDUP_MS = 1300;
/** The flight from the note's corner to the icon. */
const FLIGHT_MS = 860;

/**
 * What the glove says, in one of three shapes: an instruction (one line, the
 * keys in it: "Hold [fn] and talk."), an outcome (what happened, and what's
 * next), or a problem (what went wrong, and how to fix it). `id` names the
 * line, so a new one fades in and the same one stays put.
 */
type Say =
  | { shape: "instruction"; id: string; line: React.ReactNode; mood: Mood }
  | {
      shape: "outcome" | "problem";
      id: string;
      line: string;
      sub: React.ReactNode;
      mood: Mood;
    };

/**
 * The bubble eases to the size of what it says instead of jumping: as wide
 * as its widest line (up to a cap), as tall as its parts, so a short line
 * never sits in a long box and a line with keys and one without hand over
 * smoothly. Text that wraps would leave the box at the cap with room to its
 * right, so it's then narrowed as far as it goes without another line.
 */
const EasedSize: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const inner = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState<{ width: number; height: number }>();
  useLayoutEffect(() => {
    const el = inner.current;
    if (!el) return;
    const measure = () => {
      el.style.width = "";
      const height = el.offsetHeight;
      // offsetWidth rounds, and a box a fraction narrower than its text
      // wraps it: a pixel over always fits. (Layout sizes, not
      // getBoundingClientRect's, so the camera move's scale can't skew it.)
      let fits = el.offsetWidth + 1;
      let wraps = 0;
      while (fits - wraps > 1) {
        const mid = Math.floor((fits + wraps) / 2);
        el.style.width = `${mid}px`;
        if (el.offsetHeight > height) wraps = mid;
        else fits = mid;
      }
      el.style.width = `${fits}px`;
      setSize((was) =>
        was?.width === fits && was.height === height
          ? was
          : { width: fits, height },
      );
    };
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    // The box is pinned to its width, so content that gets narrower without
    // wrapping less (a shorter shortcut in the same line) never resizes it:
    // a change inside measures it again too.
    const mo = new MutationObserver(measure);
    mo.observe(el, { childList: true, subtree: true, characterData: true });
    measure();
    return () => {
      ro.disconnect();
      mo.disconnect();
    };
  }, []);
  return (
    <div className="try-eased" style={size}>
      <div ref={inner} className="try-eased-inner">
        {children}
      </div>
    </div>
  );
};

/**
 * The shortcut's keys, in a line of the bubble. While nothing's going on
 * they're also the way to change it: they lift under the pointer, a "Change"
 * tip shows over them (`onTip`, drawn by the bubble so its edge can't clip
 * it), and a click starts recording new ones.
 */
const LineKeys: React.FC<{
  binding: string;
  os: OSType;
  pressed: boolean;
  /** Start changing them; absent while a take's going on. */
  onChange?: () => void;
  /** The keys' element while the tip should show over them, else null. */
  onTip?: (el: HTMLElement | null) => void;
  label?: string;
}> = ({ binding, os, pressed, onChange, onTip, label }) => {
  const keys = (
    <Keycaps
      className={onChange ? "" : "try-keys"}
      binding={binding}
      osType={os}
      size="line"
      pressed={pressed}
      lift={!!onChange}
    />
  );
  if (!onChange) return keys;
  const show = (ev: React.SyntheticEvent<HTMLElement>) =>
    onTip?.(ev.currentTarget);
  const hide = () => onTip?.(null);
  return (
    <button
      type="button"
      className="try-keys try-keys-change group/keycaps"
      aria-label={label}
      // The cursor stays in the note, so the next take still lands there.
      onMouseDown={(ev) => ev.preventDefault()}
      {...spaceDoesNotClick}
      onMouseEnter={show}
      onMouseLeave={hide}
      onFocus={(ev) => ev.currentTarget.matches(":focus-visible") && show(ev)}
      onBlur={hide}
      onClick={() => {
        hide();
        onChange();
      }}
    >
      {keys}
    </button>
  );
};

interface TryItProps {
  progress: { index: number; count: number };
  os: OSType;
  /** All set: opens on the panel (a relaunch, or back from its rows). */
  finished: boolean;
  /** The model in use (on disk: Demo won't continue until it is). */
  model: CatalogModel | undefined;
  /** The model is on disk. */
  ready: boolean;
  /** The download, as Demo shows it: All set's panel, while it's going. */
  download: React.ReactNode;
  /** A take was too slow: back to Model, for a faster one. */
  onChangeModel: () => void;
  /** From All set's panel: to Welcome or Model, and straight back. */
  onEdit: (to: "welcome" | "model") => void;
  onBack: () => void;
  /** The move's under way: the flow's on All set now. */
  onArrive: () => void;
  /** Back from All set to Try it, on the same screen. */
  onUnfinish: () => void;
  /** Done, on All set: setup's over. */
  onDone: () => void;
}

/**
 * Screen 5: the first real dictation. Demo's screen, close up, with one note
 * on it that already has the cursor, and Handy's glove on the note's corner
 * with a speech bubble saying what to do. Use the shortcut and the words land
 * at the cursor, as they will in any app. Hold, then tap: either way counts
 * for itself, in either order, and one is enough to finish.
 *
 * The bubble's line is always the next thing to do, with the shortcut's keys
 * in it, so it names them. It changes once the way they're pressing is known
 * (how to stop); in between, the keys in the line press with the real ones,
 * and the glove nods along. When the keys come up, that line dims while the
 * words are on their way, and only a wait long enough to notice says "Watch
 * the note."
 */
export const TryIt: React.FC<TryItProps> = ({
  progress,
  os,
  finished,
  model,
  ready,
  download,
  onChangeModel,
  onEdit,
  onBack,
  onArrive,
  onUnfinish,
  onDone,
}) => {
  const { t, i18n } = useTranslation();
  const e = useTryEngine({ os });
  const anyDone = e.lessons.hold || e.lessons.tap;
  const { languages, atLogin, set } = useOnboardingStore();
  const { refreshAudioDevices } = useSettings();

  // The microphone list is only loaded once onboarding's done, and an empty
  // one can't be opened: load it when it's needed. (The mic's allowed by
  // now; it just recorded.)
  const silent = e.result.kind === "silent";
  useEffect(() => {
    if (silent) refreshAudioDevices();
  }, [silent, refreshAudioDevices]);
  const name = model?.name ?? "";

  const seconds = (s: number) =>
    t("onboarding.try.seconds", {
      value: new Intl.NumberFormat(i18n.language, {
        minimumFractionDigits: s < 10 ? 1 : 0,
        maximumFractionDigits: s < 10 ? 1 : 0,
      }).format(s),
    });

  /* ---------- the move into All set ---------- */

  // Owned here once mounted: `finished` only says where a fresh mount opens.
  const [stage, setStage] = useState<Stage>(finished ? "set" : "try");
  const [how, setHow] = useState<How>("five");
  // The icon catches the glove: only after the flight, not on a relaunch.
  const [caught, setCaught] = useState(false);
  // Title and line fade only once they've swapped (into All set and back),
  // not on arrival, where Demo's camera move carries them.
  const [swapped, setSwapped] = useState(false);
  const [tail, setTail] = useState<number>();
  const screen = useRef<HTMLDivElement>(null);
  const tray = useRef<HTMLSpanElement>(null);
  const glove = useRef<HTMLButtonElement>(null);

  const start = (by: How) => {
    if (stage !== "try") return;
    setHow(by);
    setSwapped(true);
    if (prefersReducedMotion()) {
      setStage("set");
      onArrive();
      return;
    }
    setStage(by === "auto" ? "windup" : "five");
  };

  // The last take lands: no click, the glove asks for five and takes it.
  const finale = e.result.kind === "finale";
  useEffect(() => {
    if (!finale) return;
    e.dismiss();
    start("auto");
  }, [finale]);

  useEffect(() => {
    if (stage !== "windup" && stage !== "five") return;
    const next = setTimeout(
      () => setStage(stage === "windup" ? "five" : "fly"),
      stage === "windup"
        ? WINDUP_MS
        : how === "skip"
          ? 220
          : how === "auto"
            ? 540
            : 440,
    );
    return () => clearTimeout(next);
  }, [stage, how]);

  // The flight: an arc from the note's corner into the icon, bowing in over
  // the desktop up to a menu bar, hopping along to a tray, a little big as it
  // arrives and gone into it as it lands.
  useLayoutEffect(() => {
    if (stage !== "fly") return;
    onArrive();
    const el = glove.current;
    const from = el?.getBoundingClientRect();
    const to = tray.current?.getBoundingClientRect();
    if (!el || !from || !to) {
      setStage("set");
      return;
    }
    const dx = to.left + to.width / 2 - (from.left + from.width / 2);
    const dy = to.top + to.height / 2 - (from.top + from.height / 2);
    const s = to.height / from.height;
    const rtl = document.documentElement.dir === "rtl";
    const up = os === "macos";
    const bow = up ? (rtl ? 70 : -70) : 0;
    const hop = up ? 0 : -70;
    const flight = el.animate(
      [
        { transform: "translate(0, 0) scale(1) rotate(0deg)", opacity: 1 },
        {
          transform: `translate(${dx * 0.45 + bow}px, ${dy * 0.5 + hop}px) scale(0.55) rotate(-18deg)`,
          opacity: 1,
          offset: 0.5,
        },
        {
          transform: `translate(${dx}px, ${dy}px) scale(${s * 1.6}) rotate(0deg)`,
          opacity: 1,
          offset: 0.86,
        },
        {
          transform: `translate(${dx}px, ${dy}px) scale(${s}) rotate(0deg)`,
          opacity: 0,
        },
      ],
      {
        duration: FLIGHT_MS,
        easing: "cubic-bezier(0.55, 0, 0.25, 1)",
        fill: "both",
      },
    );
    // The icon catches it as it lands, not after its fade, so the panel
    // opens while the room for it is still opening.
    const land = setTimeout(() => {
      setCaught(true);
      setStage("set");
    }, FLIGHT_MS * 0.88);
    return () => {
      clearTimeout(land);
      flight.cancel();
    };
  }, [stage]);

  // The panel's tail points at the icon, wherever the bar puts it.
  useLayoutEffect(() => {
    if (stage !== "set") return;
    const box = screen.current?.getBoundingClientRect();
    const icon = tray.current?.getBoundingClientRect();
    if (!box || !icon) return;
    const rtl = document.documentElement.dir === "rtl";
    const mid = icon.left + icon.width / 2;
    const fromEnd = rtl ? mid - box.left : box.right - mid;
    // Its edge and size are tryit.css's (--try-panel-*); the tail goes
    // wherever the icon is.
    const end =
      parseFloat(
        getComputedStyle(screen.current!).getPropertyValue("--try-panel-end"),
      ) || 0;
    setTail(Math.max(24, fromEnd - end));
  }, [stage, os]);

  const moving = stage === "windup" || stage === "five" || stage === "fly";
  const after = stage === "fly" || stage === "set";
  const fived = (stage === "five" || stage === "fly") && how !== "skip";

  /* ---------- changing the shortcut ---------- */

  const bubble = useRef<HTMLDivElement>(null);
  // The "Change" tip, placed over the keys from the bubble's corner.
  const [tip, setTip] = useState<{ x: number; y: number } | null>(null);
  const placeTip = (el: HTMLElement | null) => {
    const box = bubble.current?.getBoundingClientRect();
    if (!el || !box) return setTip(null);
    const keys = el.getBoundingClientRect();
    setTip({ x: keys.left + keys.width / 2 - box.left, y: keys.top - box.top });
  };
  const { capturing, partial } = e.capture;
  const partialKeys = bindingKeys(partial);
  const onlyModifiers = isOnlyModifiers(partialKeys);
  const keysLabel = keycapLabel(e.shortcut, os);

  /* ---------- what the glove says ---------- */

  const say = ((): Say => {
    // The finale: the glove asks, winds up, gives it.
    if (moving && how === "auto")
      return {
        shape: "instruction",
        id: "giveFive",
        line: t("onboarding.try.giveFive"),
        mood: "cheer",
      };
    const instruction = (key: string): Say => ({
      shape: "instruction",
      id: key,
      line: (
        <Trans
          i18nKey={key}
          components={{
            keys: (
              <LineKeys
                binding={e.shortcut}
                os={os}
                pressed={e.keysDown}
                onChange={e.phase === "idle" ? e.capture.start : undefined}
                onTip={placeTip}
                label={t("onboarding.try.changeKeys", { keys: keysLabel })}
              />
            ),
          }}
        />
      ),
      // Only read while idle: mid-take the glove listens (data-mood below).
      mood: "point",
    });
    // New keys going in: one line, the slot that takes them, and Cancel.
    if (capturing)
      return {
        shape: "instruction",
        id: onlyModifiers ? "addKey" : "pressKeys",
        line: (
          <>
            {t(
              onlyModifiers
                ? "onboarding.demo.addKey"
                : "onboarding.demo.pressKeys",
            )}{" "}
            {partialKeys.length ? (
              <Keycaps
                className="try-keys"
                binding={partial}
                osType={os}
                size="line"
                partial
              />
            ) : (
              <KeySlot size="line" className="try-keys try-slot" />
            )}
            <TextLink className="try-cancel" onClick={e.capture.cancel}>
              {t("onboarding.demo.cancel")}
            </TextLink>
          </>
        ),
        mood: "point",
      };
    // Keys up, words on their way: usually they land in a blink, so the line
    // stays (dimmed, below); only a real wait gets its own.
    if (e.phase === "typing" && e.waiting)
      return instruction("onboarding.try.watch");
    // Keys down: how to stop, for the way they're actually going. Until
    // that's known, whatever was up stays (an instruction, a cheer, a
    // problem), so the bubble changes once, straight to this, not to a
    // guess first.
    if (e.phase !== "idle" && e.take)
      return instruction(
        e.take === "tap" ? "onboarding.try.tapAgain" : "onboarding.try.letGo",
      );

    const r = e.result;
    switch (r.kind) {
      case "done":
        return r.way === "hold"
          ? {
              shape: "outcome",
              id: "doneHold",
              line: t("onboarding.try.doneHold"),
              sub: t("onboarding.try.doneHoldNext"),
              mood: "cheer",
            }
          : {
              shape: "outcome",
              id: "doneTap",
              line: t("onboarding.try.doneTap"),
              sub: t("onboarding.try.doneTapNext"),
              mood: "cheer",
            };
      // The mic sent nothing at all: Settings' microphone list, right here.
      // Picking one is the fix, so it's back to what's next, the cursor in
      // the note. The note keeps the cursor while the list is used, and the
      // list isn't read out as the bubble's news.
      case "silent":
        return {
          shape: "problem",
          id: "silent",
          line: t("onboarding.try.silent"),
          sub: (
            <>
              {t("onboarding.try.silentSub")}
              <div
                className="try-mic-pick"
                aria-live="off"
                onMouseDown={(ev) => ev.preventDefault()}
              >
                <MicrophoneDropdown
                  placement="above"
                  tone="neutral"
                  onPicked={() => {
                    e.dismiss();
                    e.field.current?.focus({ preventScroll: true });
                  }}
                />
              </div>
            </>
          ),
          mood: "oops",
        };
      case "nothingTyped":
      case "noWords":
        return {
          shape: "problem",
          id: r.kind,
          line: t(`onboarding.try.${r.kind}`),
          sub: t(`onboarding.try.${r.kind}Sub`),
          mood: "oops",
        };
    }
    // Both done before (back from All set, or a relaunch): the glove asks
    // for five, and the button gives it.
    if (!e.todo)
      return {
        shape: "outcome",
        id: "ask",
        line: t("onboarding.try.giveFive"),
        sub: (
          <Button
            variant="primary"
            size="md"
            className="try-five"
            // The cursor stays in the note.
            onMouseDown={(ev) => ev.preventDefault()}
            onClick={() => start("five")}
            disabled={stage !== "try"}
          >
            <TrayIcon state="idle" size={16} />
            {t("onboarding.try.highFive")}
          </Button>
        ),
        mood: "ready",
      };
    return instruction(
      e.todo === "hold" ? "onboarding.try.hold" : "onboarding.try.tapNext",
    );
  })();

  // The tip goes with the keys: a new line, a take or recording ends it.
  useEffect(() => setTip(null), [say.id, e.phase, capturing]);

  // The footer's button is the way out, named for how far they've got: Skip
  // before either way (someone whose dictation can't work here yet can still
  // finish setting up), Finish after one. With both done the glove asks for
  // five, and the bubble has the only button. On All set, Done.
  const hint = after ? (
    // Linux: a pass here doesn't promise the shortcut or pasting work in
    // other apps.
    os === "linux" ? (
      <LinuxGuide i18nKey="onboarding.linux.done" />
    ) : (
      t("onboarding.done.hint")
    )
  ) : !anyDone && os === "linux" ? (
    <LinuxGuide i18nKey="onboarding.linux.tryIt" />
  ) : (
    ""
  );
  const primary = after ? (
    <Button variant="primary" size="lg" onClick={onDone}>
      {t("onboarding.done.finish")}
    </Button>
  ) : stage !== "try" || !e.todo ? null : (
    <Button
      variant="outline"
      size="lg"
      onClick={() => start(anyDone ? "five" : "skip")}
    >
      {t(anyDone ? "onboarding.try.finish" : "onboarding.try.skip")}
    </Button>
  );

  const isMac = os === "macos";

  return (
    <OnboardingShell
      progress={progress}
      onBack={
        stage === "set"
          ? () => {
              setStage("try");
              setCaught(false);
              onUnfinish();
            }
          : stage === "try"
            ? onBack
            : undefined
      }
      hint={hint}
      primary={primary}
    >
      <div className="try-main" data-stage={stage}>
        {/* Title and line swap with a fade as the glove leaves; the line
            keeps its height, so the screen under it never jumps. */}
        <ScreenTitle
          key={after ? "done" : "try"}
          className={swapped ? "try-swap" : ""}
        >
          {t(after ? "onboarding.done.title" : "onboarding.try.title")}
        </ScreenTitle>
        {/* Where to look: the bubble says each thing to do when it's time,
            but it's down in the corner, far from where reading starts. On
            All set, where Handy went and how to use it anywhere. */}
        <ScreenSub
          key={after ? "done-sub" : "try-sub"}
          className={swapped ? "try-swap" : ""}
        >
          {after ? (
            <Trans
              i18nKey={
                isMac ? "onboarding.done.subMac" : "onboarding.done.subTray"
              }
              components={{
                keys: (
                  <Keycaps
                    className="try-sub-keys"
                    binding={e.shortcut}
                    osType={os}
                    size="line"
                    pressed={e.keysDown}
                  />
                ),
              }}
            />
          ) : (
            t("onboarding.try.subtitle")
          )}
        </ScreenSub>
        {/* Demo's screen, at Demo's proportions, without the laptop. */}
        <div
          ref={screen}
          className="try-screen"
          data-os={isMac ? "mac" : "win"}
        >
          {/* Handy's icon, in the menu bar or the tray, as on Demo: an ear
              while it listens, a brain while it transcribes. It catches the
              glove. */}
          <DesktopBar
            os={os}
            size="try"
            state={
              e.phase === "listening"
                ? "recording"
                : e.phase === "typing"
                  ? "transcribing"
                  : "idle"
            }
            trayRef={tray}
            caught={caught}
          />
          <div
            className="try-notes"
            // A click anywhere in the window puts the cursor in the note.
            onMouseDown={(ev) => {
              if (ev.target === e.field.current) return;
              ev.preventDefault();
              e.field.current?.focus();
            }}
          >
            <WindowBar size="lg" title={t("onboarding.demo.apps.notes")} />
            <div className="try-note">
              <p className="try-note-title">{t("onboarding.demo.noteTitle")}</p>
              <textarea
                ref={e.field}
                className="try-note-page"
                placeholder={t("onboarding.try.placeholder")}
                aria-label={t("onboarding.try.noteLabel")}
                spellCheck={false}
              />
            </div>
          </div>

          {/* Handy, on the note's corner, until it flies to the icon. */}
          {stage !== "set" && (
            <div className="try-guide">
              <button
                ref={glove}
                type="button"
                className="try-glove"
                // Nodding along from the moment the keys go down.
                data-mood={
                  stage === "windup"
                    ? "windup"
                    : moving
                      ? "cheer"
                      : e.phase === "idle"
                        ? say.mood
                        : "listen"
                }
                data-how={how}
                aria-label={t("onboarding.done.highFiveLabel")}
                // A high five leaves the cursor in the note.
                onMouseDown={(ev) => ev.preventDefault()}
                // Asked for five, a high five finishes; otherwise it's just one.
                onClick={() =>
                  stage === "try" && !e.todo ? start("five") : e.slap()
                }
              >
                <span className="try-pose">
                  <Mascot
                    key={fived ? "finale" : e.fives}
                    size={88}
                    animation={fived || e.fives ? "slap" : "wave"}
                  />
                </span>
                {(fived || e.fives > 0) && (
                  <span
                    key={fived ? "burst-finale" : `burst-${e.fives}`}
                    aria-hidden="true"
                    className="pointer-events-none absolute start-1/2 -top-[22px] -ms-[20px] animate-burst opacity-0"
                  >
                    <svg
                      viewBox="0 0 60 60"
                      width="40"
                      height="40"
                      className="block fill-none stroke-background-ui"
                      strokeWidth="4"
                      strokeLinecap="round"
                    >
                      <path d="M30 22V6M40 26l10-10M20 26 10 16" />
                    </svg>
                  </span>
                )}
              </button>
              <div
                ref={bubble}
                className={`try-bubble ${stage === "five" || stage === "fly" ? "try-leaving" : ""}`}
                data-shape={say.shape}
                role="status"
                aria-live="polite"
              >
                <EasedSize>
                  <div
                    className="try-say"
                    // Handy's turn: what they did stays, quieter.
                    data-dim={(e.phase === "typing" && !e.waiting) || undefined}
                  >
                    {/* A new line fades in; the same one stays put, so the
                      keys in it press without remounting. */}
                    <p key={`line:${say.id}`} className="try-say-main">
                      {say.line}
                    </p>
                    {say.shape !== "instruction" && (
                      <div
                        key={`sub:${say.id}`}
                        className={`try-say-sub ${say.id === "ask" ? "try-ask" : ""}`}
                      >
                        {say.sub}
                      </div>
                    )}
                  </div>
                </EasedSize>
                {tip && (
                  <span
                    className="try-tip"
                    aria-hidden="true"
                    style={{ left: tip.x, top: tip.y }}
                  >
                    {t("onboarding.demo.change")}
                  </span>
                )}
              </div>
            </div>
          )}

          {/* All set: the icon's panel, the bubble's material come back. */}
          {stage === "set" && tail != null && (
            <TrayPanel
              os={os}
              languages={languages}
              modelName={name}
              ready={ready}
              download={download}
              shortcut={e.shortcut}
              keysDown={e.keysDown}
              capture={e.capture}
              atLogin={atLogin}
              onAtLogin={(on) => set({ atLogin: on })}
              onChangeLanguages={() => onEdit("welcome")}
              onChangeModel={() => onEdit("model")}
              tailAt={tail}
            />
          )}
        </div>
        {/* Under the screen: a suggestion, not a problem, so not the glove's.
            Its height is always kept, so the screen never jumps for it. */}
        <div className="try-after">
          {e.slowWait != null && (
            <span>
              <Trans
                i18nKey="onboarding.try.slowLine"
                values={{ name, wait: seconds(e.slowWait) }}
                // Not <link>: a void element in HTML, so its words would fall out.
                components={{
                  // From All set, Model comes straight back here.
                  faster: (
                    <TextLink
                      onClick={after ? () => onEdit("model") : onChangeModel}
                    />
                  ),
                }}
              />
            </span>
          )}
        </div>
      </div>
    </OnboardingShell>
  );
};
