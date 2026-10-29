import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import {
  keycapLabel,
  type OSType,
  spaceDoesNotClick,
} from "@/lib/utils/keyboard";
import { prefersReducedMotion } from "@/lib/utils/motion";
import { Button } from "../../ui/Button";
import { TextLink } from "../../ui/TextLink";
import { CapturedKeys, captureHintKey, Keycaps } from "../../shared/Keycaps";
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
import type { CatalogModel } from "@/lib/models/catalog";
import "./tryit.css";

type Mood = "point" | "listen" | "cheer" | "oops" | "ready" | "windup";

/**
 * Moving into All set: wind up (auto only), high five (not Skip), fly to the
 * tray icon, open its panel.
 */
type Stage = "try" | "windup" | "five" | "fly" | "set";
/** What started it: the last take, the High five button (or Finish), Skip. */
type How = "auto" | "five" | "skip";

/** "Give me five" is up this long, the glove winding up, before it's given. */
const WINDUP_MS = 950;
/** The flight from the note's corner into the icon. */
const FLIGHT_MS = 560;
/** How far through the flight the glove starts fading into the icon. */
const FADE_FROM = 0.7;

/** The bubble's content. A new `id` fades in; the same `id` stays mounted. */
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
 * Animates to its content's size. Wrapped text is narrowed as far as it goes
 * without adding a line.
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
      // offsetWidth rounds down, so +1 guarantees no wrap. Layout sizes, not
      // getBoundingClientRect, so the transition's scale can't skew them.
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
    // The pinned width hides content shrinking: remeasure on DOM changes too.
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

/** The shortcut's keys in a bubble line; when idle, click to record new ones. */
const LineKeys: React.FC<{
  binding: string;
  os: OSType;
  pressed: boolean;
  /** Start changing them; absent while a take's going on. */
  onChange?: () => void;
  /** Tip anchor; drawn by the bubble so its edge can't clip it. */
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
  /** Open directly on All set. */
  finished: boolean;
  model: CatalogModel | undefined;
  /** The model is on disk. */
  ready: boolean;
  /** Download status, shown in All set's panel while in progress. */
  download: React.ReactNode;
  /** A take was too slow: go pick a faster model. */
  onChangeModel: () => void;
  /** Edit from All set's panel, then return here. */
  onEdit: (to: "welcome" | "model") => void;
  onBack: () => void;
  onArrive: () => void;
  onUnfinish: () => void;
  onDone: () => void;
}

/** Screen 5: first real dictation into a note; one take (hold or tap) ends it. */
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

  // The mic list isn't loaded until onboarding ends; load it for the picker.
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
  // Only after the flight, not on a relaunch.
  const [caught, setCaught] = useState(false);
  // Fade the title only after a swap; on arrival Demo's transition carries it.
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

  // The last take finishes it automatically.
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
      // Take off on the slap; its settle plays during the flight.
      stage === "windup" ? WINDUP_MS : how === "skip" ? 220 : 260,
    );
    return () => clearTimeout(next);
  }, [stage, how]);

  // Fly the glove into the tray icon: straight up on macOS, else a hop down.
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
    const at = (x: number, y: number, scale: number) =>
      `translate(${x}px, ${y}px) scale(${scale})`;
    const legs: Keyframe[] =
      os === "macos"
        ? [
            {
              transform: at(0, 0, 1),
              opacity: 1,
              easing: "cubic-bezier(0.35, 0, 0.15, 1)",
            },
          ]
        : [
            {
              transform: at(0, 0, 1),
              opacity: 1,
              easing: "cubic-bezier(0.33, 1, 0.68, 1)",
            },
            {
              transform: at(dx * 0.4, Math.min(dy, 0) - 40, 0.7),
              opacity: 1,
              offset: 0.32,
              easing: "cubic-bezier(0.32, 0, 0.67, 0)",
            },
          ];
    const flight = el.animate(
      [
        ...legs,
        { opacity: 1, offset: FADE_FROM },
        { transform: at(dx, dy, s), opacity: 0 },
      ],
      { duration: FLIGHT_MS, fill: "both" },
    );
    const land = setTimeout(() => {
      setCaught(true);
      setStage("set");
    }, FLIGHT_MS);
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
    // The panel's inset comes from tryit.css (--try-panel-end).
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
  const captureHint = captureHintKey(partial);
  const keysLabel = keycapLabel(e.shortcut, os);

  /* ---------- what the glove says ---------- */

  const say = ((): Say => {
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
      // Only used while idle (see data-mood).
      mood: "point",
    });
    if (capturing)
      return {
        shape: "instruction",
        id: captureHint,
        line: (
          <>
            {t(captureHint)}{" "}
            <CapturedKeys
              partial={partial}
              osType={os}
              size="line"
              className="try-keys"
              slotClassName="try-slot"
            />
            <TextLink className="try-cancel" onClick={e.capture.cancel}>
              {t("onboarding.demo.cancel")}
            </TextLink>
          </>
        ),
        mood: "point",
      };
    // Only a noticeable wait replaces the (dimmed) line.
    if (e.phase === "typing" && e.waiting)
      return instruction("onboarding.try.watch");
    // Keys down: wait until hold vs. tap is known so the bubble changes once.
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
      // Silent mic: offer the picker inline, keeping focus in the note.
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
    // Both already done (back from All set, or a relaunch).
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

  // Hide the tip on a new line, a take, or capture.
  useEffect(() => setTip(null), [say.id, e.phase, capturing]);

  // Skip before any take (dictation may not work here yet), Finish after one,
  // none once both are done (the bubble has the button), Done on All set.
  const hint = after ? (
    // On Linux, working here doesn't guarantee it works in other apps.
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
        <ScreenTitle
          key={after ? "done" : "try"}
          className={swapped ? "try-swap" : ""}
        >
          {t(after ? "onboarding.done.title" : "onboarding.try.title")}
        </ScreenTitle>
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
        <div
          ref={screen}
          className="try-screen"
          data-os={isMac ? "mac" : "win"}
        >
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

          {stage !== "set" && (
            <div className="try-guide">
              <button
                ref={glove}
                type="button"
                className="try-glove"
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
                onMouseDown={(ev) => ev.preventDefault()}
                // Finishes when both ways are done; otherwise just a slap.
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
                    data-dim={(e.phase === "typing" && !e.waiting) || undefined}
                  >
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
        {/* Always reserves its height so the screen doesn't jump. */}
        <div className="try-after">
          {e.slowWait != null && (
            <span>
              <Trans
                i18nKey="onboarding.try.slowLine"
                values={{ name, wait: seconds(e.slowWait) }}
                // Not <link>: it's a void element in HTML.
                components={{
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
