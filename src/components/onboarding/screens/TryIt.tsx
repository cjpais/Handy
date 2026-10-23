import React, { useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { OSType } from "@/lib/utils/keyboard";
import { Button } from "../../ui/Button";
import { TextLink } from "../../ui/TextLink";
import { Keycaps } from "../../shared/Keycaps";
import { Mascot } from "../../shared/Mascot";
import { ProgressLine } from "../../shared/ProgressLine";
import HandyHand from "../../icons/HandyHand";
import { OnboardingShell, ScreenSub, ScreenTitle } from "../OnboardingShell";
import { useTryEngine } from "../useTryEngine";
import type { CatalogModel } from "../data/models";
import type { DownloadState } from "../useOnboardingModels";
import "./tryit.css";

type Mood = "point" | "listen" | "cheer" | "oops" | "wait";

/** One thing the glove says. A new `id` is a new line (it fades in). */
interface Say {
  id: string;
  main: React.ReactNode;
  sub?: React.ReactNode;
  /** The shortcut, under the line, with the listening status beside it. */
  keys?: boolean;
  mood: Mood;
}

/**
 * The bubble eases to the height of what it says instead of jumping, so a
 * line with keys and one without hand over smoothly.
 */
const EasedHeight: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const inner = useRef<HTMLDivElement>(null);
  const [height, setHeight] = useState<number | undefined>(undefined);
  useLayoutEffect(() => {
    const el = inner.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setHeight(el.offsetHeight));
    ro.observe(el);
    setHeight(el.offsetHeight);
    return () => ro.disconnect();
  }, []);
  return (
    <div className="try-eased" style={{ height }}>
      <div ref={inner}>{children}</div>
    </div>
  );
};

interface TryItProps {
  progress: { index: number; count: number };
  os: OSType;
  /** The model in use (being downloaded or on disk). */
  model: CatalogModel | undefined;
  download: DownloadState;
  /** A take was too slow: back to Model, for a faster one. */
  onChangeModel: () => void;
  onBack: () => void;
  onFinish: () => void;
}

/**
 * Screen 5: the first real dictation. Demo's screen, close up, with one note
 * on it, and Handy's glove on the note's corner with a speech bubble saying
 * what to do next. Click into the note, use the shortcut, and the words land
 * at the cursor, as they will in any app. Hold, then tap: either way counts
 * for itself, and one is enough to finish.
 *
 * The rule for the bubble: while you're doing what it says, it doesn't
 * change. Pressing the keys presses them on screen, and a status beside them
 * says Listening, then Typing; the words only change for an outcome (a high
 * five, a slip, a problem) or the next thing to do.
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
  const ready = download.state === "ready";
  const e = useTryEngine({ ready, os });
  const anyDone = e.ways.some((w) => e.lessons[w]);
  const allDone = e.ways.every((w) => e.lessons[w]);
  const both = e.ways.length > 1;
  const name = model?.name ?? "";
  const busy = e.phase !== "idle";

  const seconds = (s: number) =>
    t("onboarding.model.seconds", {
      value: new Intl.NumberFormat(i18n.language, {
        minimumFractionDigits: s < 10 ? 1 : 0,
        maximumFractionDigits: s < 10 ? 1 : 0,
      }).format(s),
    });
  const eta =
    download.state === "downloading" && download.etaSeconds != null
      ? download.etaSeconds < 60
        ? t("onboarding.download.seconds", {
            count: Math.ceil(download.etaSeconds),
          })
        : t("onboarding.download.minutes", {
            count: Math.ceil(download.etaSeconds / 60),
          })
      : null;

  /* ---------- what the glove says ---------- */

  const instruction = (): Say => {
    if (allDone)
      return {
        id: "allDone",
        main: t(both ? "onboarding.try.allDone" : "onboarding.try.allDoneOne"),
        sub: t("onboarding.try.allDoneSub"),
        mood: "point",
      };
    if (!e.clicked)
      return {
        id: "click",
        main: t(anyDone ? "onboarding.try.clickBack" : "onboarding.try.click"),
        mood: "point",
      };
    return e.lesson === "hold"
      ? {
          id: "hold",
          main: t("onboarding.try.hold"),
          keys: true,
          sub: t("onboarding.try.holdSub"),
          mood: "point",
        }
      : {
          id: "tap",
          main: t(anyDone ? "onboarding.try.tapNext" : "onboarding.try.tap"),
          keys: true,
          sub: t("onboarding.try.tapSub"),
          mood: "point",
        };
  };

  const say = ((): Say => {
    if (!ready)
      return download.state === "failed"
        ? {
            id: "downloadFailed",
            main: t("onboarding.try.downloadFailed", { name }),
            sub: t("onboarding.try.downloadFailedSub"),
            mood: "oops",
          }
        : {
            id: "downloading",
            main: t("onboarding.try.gettingReady"),
            sub: (
              <>
                <ProgressLine
                  value={
                    download.state === "downloading" ? download.progress : 1
                  }
                  className="mt-[6px] mb-[6px] h-[5px]"
                />
                <span className="tabular-nums">
                  {download.state === "checking"
                    ? t("onboarding.try.checking", { name })
                    : eta
                      ? t("onboarding.try.downloadingEta", { name, time: eta })
                      : t("onboarding.try.downloading", { name })}
                </span>
              </>
            ),
            mood: "wait",
          };

    // Mid-take the line stays what it was. The one exception: a tap where a
    // hold was asked for, because Handy won't stop until the next tap.
    if (busy) {
      if (e.lesson === "hold" && e.take === "tap")
        return {
          id: "tapInHold",
          main: t("onboarding.try.tapInHold"),
          keys: true,
          sub: t("onboarding.try.tapInHoldSub"),
          mood: "listen",
        };
      return { ...instruction(), mood: "listen" };
    }

    const r = e.result;
    switch (r.kind) {
      case "done":
        return r.way === "hold"
          ? {
              id: "doneHold",
              main: t("onboarding.try.doneHold"),
              sub:
                both && !e.lessons.tap
                  ? t("onboarding.try.doneHoldNext")
                  : allDone
                    ? t("onboarding.try.readySub")
                    : undefined,
              mood: "cheer",
            }
          : {
              id: "doneTap",
              main: t(
                allDone && both
                  ? "onboarding.try.doneBoth"
                  : "onboarding.try.doneTap",
              ),
              sub: allDone ? t("onboarding.try.readySub") : undefined,
              mood: "cheer",
            };
      case "credited":
        return r.way === "tap"
          ? {
              id: "creditedTap",
              main: t("onboarding.try.creditedTap"),
              keys: true,
              sub: t("onboarding.try.creditedTapSub"),
              mood: "cheer",
            }
          : {
              id: "creditedHold",
              main: t("onboarding.try.creditedHold"),
              keys: true,
              sub: t("onboarding.try.creditedHoldSub"),
              mood: "cheer",
            };
      case "slow":
        return {
          id: "slow",
          main: t("onboarding.try.slow", { wait: seconds(r.wait) }),
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
          id: r.kind,
          main: t(`onboarding.try.${r.kind}`),
          sub: t(`onboarding.try.${r.kind}Sub`),
          mood: "oops",
        };
    }
    return instruction();
  })();

  // Beside the keys: what Handy's doing, in a slot that's always there.
  const status =
    e.phase === "listening"
      ? t("onboarding.try.listening")
      : e.phase === "typing"
        ? t("onboarding.try.typing")
        : null;

  // Once the model is ready, someone whose dictation can't work here yet can
  // still finish setting up; after one way, the other is optional.
  const hint = !ready ? (
    ""
  ) : !anyDone ? (
    <TextLink onClick={onFinish}>{t("onboarding.try.skip")}</TextLink>
  ) : !allDone ? (
    t(
      e.ways.find((w) => !e.lessons[w]) === "tap"
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
                {...e.fieldProps}
              />
            </div>
          </div>

          {/* Handy, on the note's corner. */}
          <div className="try-guide">
            <button
              type="button"
              className="try-glove"
              data-mood={say.mood}
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
            <div className="try-bubble" role="status" aria-live="polite">
              <EasedHeight>
                <div key={say.id} className="try-say">
                  <p className="try-say-main">{say.main}</p>
                  {say.keys && (
                    <div className="try-say-keys">
                      <Keycaps
                        binding={e.shortcut}
                        osType={os}
                        pressed={e.keysDown}
                      />
                      <span
                        className="try-status"
                        data-on={status || undefined}
                      >
                        <i
                          aria-hidden="true"
                          data-typing={e.phase === "typing" || undefined}
                        />
                        {status ?? t("onboarding.try.listening")}
                      </span>
                    </div>
                  )}
                  {say.sub && <div className="try-say-sub">{say.sub}</div>}
                </div>
              </EasedHeight>
            </div>
          </div>
        </div>
      </div>
    </OnboardingShell>
  );
};
