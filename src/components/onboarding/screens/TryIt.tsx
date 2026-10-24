import React, { useLayoutEffect, useRef, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import type { OSType } from "@/lib/utils/keyboard";
import { Button } from "../../ui/Button";
import { TextLink } from "../../ui/TextLink";
import { Keycaps } from "../../shared/Keycaps";
import { Mascot } from "../../shared/Mascot";
import HandyHand from "../../icons/HandyHand";
import { OnboardingShell, ScreenSub, ScreenTitle } from "../OnboardingShell";
import { useTryEngine } from "../useTryEngine";
import type { CatalogModel } from "../data/models";
import type { DownloadState } from "../useOnboardingModels";
import "./tryit.css";

type Mood = "point" | "listen" | "cheer" | "oops";

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
    measure();
    return () => ro.disconnect();
  }, []);
  return (
    <div className="try-eased" style={size}>
      <div ref={inner} className="try-eased-inner">
        {children}
      </div>
    </div>
  );
};

interface TryItProps {
  progress: { index: number; count: number };
  os: OSType;
  /** The model in use (on disk: Demo won't continue until it is). */
  model: CatalogModel | undefined;
  download: DownloadState;
  /** A take was too slow: back to Model, for a faster one. */
  onChangeModel: () => void;
  onBack: () => void;
  onFinish: () => void;
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
  model,
  download,
  onChangeModel,
  onBack,
  onFinish,
}) => {
  const { t, i18n } = useTranslation();
  const e = useTryEngine({ ready: download.state === "ready", os });
  const anyDone = e.lessons.hold || e.lessons.tap;
  const name = model?.name ?? "";

  const seconds = (s: number) =>
    t("onboarding.model.seconds", {
      value: new Intl.NumberFormat(i18n.language, {
        minimumFractionDigits: s < 10 ? 1 : 0,
        maximumFractionDigits: s < 10 ? 1 : 0,
      }).format(s),
    });

  /* ---------- what the glove says ---------- */

  const say = ((): Say => {
    const instruction = (key: string): Say => ({
      shape: "instruction",
      id: key,
      line: (
        <Trans
          i18nKey={key}
          components={{
            keys: (
              <Keycaps
                className="try-keys"
                binding={e.shortcut}
                osType={os}
                size="line"
                pressed={e.keysDown}
              />
            ),
          }}
        />
      ),
      mood: e.phase === "idle" ? "point" : "listen",
    });
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
      case "slow":
        return {
          shape: "problem",
          id: "slow",
          line: t("onboarding.try.slow", { wait: seconds(r.wait) }),
          sub: (
            <>
              {t("onboarding.try.slowSub", { name })}{" "}
              <TextLink onClick={onChangeModel}>
                {t("onboarding.try.pickFaster")}
              </TextLink>
            </>
          ),
          mood: "oops",
        };
      case "nothingTyped":
      case "empty":
      case "failed":
        return {
          shape: "problem",
          id: r.kind,
          line: t(`onboarding.try.${r.kind}`),
          sub: t(`onboarding.try.${r.kind}Sub`),
          mood: "oops",
        };
    }
    // Both done: the finale, until the next press.
    if (!e.todo)
      return {
        shape: "outcome",
        id: "allDone",
        line: t("onboarding.try.allDone"),
        sub: t("onboarding.try.allDoneSub"),
        mood: "cheer",
      };
    return instruction(
      e.todo === "hold" ? "onboarding.try.hold" : "onboarding.try.tapNext",
    );
  })();

  // Someone whose dictation can't work here yet can still finish setting
  // up; after one way, the other is optional.
  const hint = !anyDone ? (
    <TextLink onClick={onFinish}>{t("onboarding.try.skip")}</TextLink>
  ) : e.todo ? (
    t(
      e.todo === "tap"
        ? "onboarding.try.finishOrTap"
        : "onboarding.try.finishOrHold",
    )
  ) : (
    ""
  );

  const isMac = os === "macos";
  // Handy's icon, in the menu bar or the tray: pink while it listens.
  const tray = (
    <span className="try-tray" data-on={e.phase === "listening" || undefined}>
      <HandyHand width={13} height={14} tone="current" />
    </span>
  );
  const clock = <span className="tabular-nums">9:41</span>;

  return (
    <OnboardingShell
      progress={progress}
      onBack={onBack}
      hint={hint}
      primary={
        <Button
          variant="primary"
          size="lg"
          disabled={!e.preview && !anyDone}
          onClick={onFinish}
        >
          {t("onboarding.try.finish")}
        </Button>
      }
    >
      <div className="try-main">
        <ScreenTitle>{t("onboarding.try.title")}</ScreenTitle>
        {/* Where to look: the bubble says each thing to do when it's time,
            but it's down in the corner, far from where reading starts. */}
        <ScreenSub>{t("onboarding.try.subtitle")}</ScreenSub>
        {/* Demo's screen, at Demo's proportions, without the laptop. */}
        <div className="try-screen" data-os={isMac ? "mac" : "win"}>
          {isMac ? (
            <div className="try-bar is-menu" aria-hidden="true">
              <span className="try-apple" />
              <i className="w-[30px]" />
              <i className="w-[22px]" />
              <i className="w-[26px]" />
              <span className="flex-1" />
              {tray}
              {clock}
            </div>
          ) : (
            <div className="try-bar is-task" aria-hidden="true">
              <span className="flex-1" />
              <span className="try-start">
                {[0, 1, 2, 3].map((i) => (
                  <i key={i} />
                ))}
              </span>
              {[0, 1, 2].map((i) => (
                <b key={i} />
              ))}
              <span className="flex-1" />
              {tray}
              {clock}
            </div>
          )}
          <div
            className="try-notes"
            // A click anywhere in the window puts the cursor in the note.
            onMouseDown={(ev) => {
              if (ev.target === e.field.current) return;
              ev.preventDefault();
              e.field.current?.focus();
            }}
          >
            <div className="try-notes-bar">
              <span className="flex gap-[6px]" aria-hidden="true">
                <i />
                <i />
                <i />
              </span>
              <span className="absolute inset-0 grid place-items-center">
                {t("onboarding.demo.apps.notes")}
              </span>
            </div>
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

          {/* Handy, on the note's corner. */}
          <div className="try-guide">
            <button
              type="button"
              className="try-glove"
              // Nodding along from the moment the keys go down.
              data-mood={e.phase === "idle" ? say.mood : "listen"}
              aria-label={t("onboarding.done.highFiveLabel")}
              // A high five leaves the cursor in the note.
              onMouseDown={(ev) => ev.preventDefault()}
              onClick={e.slap}
            >
              <span className="try-pose">
                <Mascot
                  key={e.fives}
                  size={88}
                  animation={e.fives ? "slap" : "wave"}
                />
              </span>
              {e.fives > 0 && (
                <span
                  key={`burst-${e.fives}`}
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
              className="try-bubble"
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
                    <div key={`sub:${say.id}`} className="try-say-sub">
                      {say.sub}
                    </div>
                  )}
                </div>
              </EasedSize>
            </div>
          </div>
        </div>
      </div>
    </OnboardingShell>
  );
};
