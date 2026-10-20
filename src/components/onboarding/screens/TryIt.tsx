import React, { useEffect, useRef, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { Window } from "@tauri-apps/api/window";
import { commands, events } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { useShortcutPress } from "@/hooks/useShortcutPress";
import type { OSType } from "@/lib/utils/keyboard";
import { formatModelSize } from "@/lib/utils/format";
import {
  PRACTICE_NOTE_CLOSED_EVENT,
  PRACTICE_NOTE_FOCUS_EVENT,
  PRACTICE_NOTE_LABEL,
} from "@/note/events";
import { Button } from "../../ui/Button";
import { TextLink } from "../../ui/TextLink";
import { Keycaps } from "../../shared/Keycaps";
import { OnboardingShell, ScreenSub, ScreenTitle } from "../OnboardingShell";
import { useOnboardingStore } from "../onboardingStore";
import { useSuspendedShortcuts } from "../useSuspendedShortcuts";
import {
  DICTATION_SECONDS,
  fasterOption,
  isTooSlow,
  type CatalogModel,
} from "../data/models";
import type {
  DownloadState,
  useOnboardingModels,
} from "../useOnboardingModels";

// Closing the note waits a tick: when the screen mounts again straight away
// (React's StrictMode, or a language change re-running the effect) the note
// stays open instead of being destroyed under the new open.
let noteCloseTimer: ReturnType<typeof setTimeout> | undefined;

/** Asks the note's window itself, rather than trusting focus events seen. */
const isNoteFocused = async () => {
  try {
    const note = await Window.getByLabel(PRACTICE_NOTE_LABEL);
    return !!note && (await note.isFocused());
  } catch {
    return false;
  }
};

type Mode = "hold" | "tap";
type Result =
  | { kind: "none" }
  | { kind: "notYet" }
  | { kind: "nothingTyped" }
  | { kind: "empty" }
  | { kind: "failed" }
  | { kind: "noteClosed" }
  | { kind: "short"; wait: number }
  | { kind: "ok"; speech: number; wait: number }
  | { kind: "slow"; speech: number; wait: number }
  | { kind: "switched" }
  | { kind: "switchedReady" };

interface TryItProps {
  progress: { index: number; count: number };
  pill?: React.ReactNode;
  os: OSType;
  models: ReturnType<typeof useOnboardingModels>;
  /** The model in use (being downloaded or on disk). */
  model: CatalogModel | undefined;
  download: DownloadState;
  /** Download a quicker model (the slow-take offer). */
  onSwitch: (modelId: string) => void;
  onBack: () => void;
  onFinish: () => void;
}

/**
 * Screen 5: the first real dictation. A blank note opens beside Handy; click
 * into it, hold or tap the shortcut and talk, and the words land at the
 * cursor, as they will in any app. Takes of 5 seconds or more are timed, and
 * a slow one offers a quicker model.
 */
export const TryIt: React.FC<TryItProps> = ({
  progress,
  pill,
  os,
  models,
  model,
  download,
  onSwitch,
  onBack,
  onFinish,
}) => {
  const { t, i18n } = useTranslation();
  const { getSetting } = useSettings();
  const { languages, preview, lessons, setLessons } = useOnboardingStore();
  const shortcut = getSetting("bindings")?.transcribe?.current_binding ?? "";
  const threshold = getSetting("hold_threshold_ms") ?? 300;
  const ready = preview || download.state === "ready";

  const [clicked, setClicked] = useState(false);
  const [mode, setMode] = useState<Mode | null>(null);
  const [finished, setFinished] = useState(false);
  const [keysDown, setKeysDown] = useState(false);
  const [result, setResult] = useState<Result>({ kind: "none" });
  const [switchedTo, setSwitchedTo] = useState<string | null>(null);
  const focusedRef = useRef(false);
  const modeRef = useRef<Mode | null>(null);
  const lockedRef = useRef(false);
  const holdTimer = useRef<ReturnType<typeof setTimeout>>();

  const name = model?.name ?? "";
  const setModeBoth = (m: Mode | null) => {
    modeRef.current = m;
    setMode(m);
  };

  /* ---------- the note, and Handy's input ---------- */

  const openNote = () =>
    commands
      .openPracticeNote(t("onboarding.try.noteTitle"))
      .catch(console.error);

  useEffect(() => {
    if (preview) return;
    clearTimeout(noteCloseTimer);
    openNote();
    commands.initializeEnigo().catch(console.error);
    return () => {
      noteCloseTimer = setTimeout(() => {
        commands.closePracticeNote().catch(console.error);
      }, 0);
    };
  }, [preview, t]);

  // The shortcut records only once there's a model to transcribe with.
  useSuspendedShortcuts(!preview && !ready);
  useEffect(() => {
    if (preview || !ready) return;
    commands
      .initializeShortcuts()
      .then(() => commands.resumeAllBindings())
      .catch(console.error);
  }, [preview, ready]);

  useEffect(() => {
    const unlisten = listen<{ focused: boolean }>(
      PRACTICE_NOTE_FOCUS_EVENT,
      ({ payload }) => {
        focusedRef.current = payload.focused;
        if (payload.focused) {
          setClicked(true);
          setResult((r) => (r.kind === "noteClosed" ? { kind: "none" } : r));
        }
      },
    );
    // Already in front (this screen came back, or its event came first).
    isNoteFocused().then((on) => {
      if (!on) return;
      focusedRef.current = true;
      setClicked(true);
    });
    // Closed from its own title bar, or with Handy's window: start over at
    // "click into the note", with a way to open it again.
    const unlistenClosed = listen(PRACTICE_NOTE_CLOSED_EVENT, () => {
      focusedRef.current = false;
      setClicked(false);
      setResult({ kind: "noteClosed" });
    });
    return () => {
      unlisten.then((off) => off());
      unlistenClosed.then((off) => off());
    };
  }, []);

  /* ---------- takes ---------- */

  useEffect(() => {
    const offState = events.recordingStateEvent.listen(({ payload }) => {
      const phase = payload.phase;
      if (phase === "started") {
        lockedRef.current = false;
        setKeysDown(true);
        setFinished(false);
        clearTimeout(holdTimer.current);
        // Still going at the threshold without locking on: they're holding.
        holdTimer.current = setTimeout(() => {
          if (!lockedRef.current) setModeBoth("hold");
        }, threshold);
      } else if (phase === "locked") {
        lockedRef.current = true;
        clearTimeout(holdTimer.current);
        setKeysDown(false);
        setModeBoth("tap");
      } else {
        clearTimeout(holdTimer.current);
        setKeysDown(false);
        if (phase === "cancelled") setModeBoth(null);
        else if (!modeRef.current)
          setModeBoth(lockedRef.current ? "tap" : "hold");
      }
    });
    const offDone = events.transcriptionCompleteEvent.listen(
      async ({ payload }) => {
        const how = modeRef.current ?? "hold";
        if (payload.outcome !== "typed") {
          setModeBoth(null);
          setResult({ kind: payload.outcome });
          return;
        }
        if (!focusedRef.current && !(await isNoteFocused())) {
          setModeBoth(null);
          setResult({ kind: "nothingTyped" });
          return;
        }
        setFinished(true);
        setLessons({ [how]: true });
        const speech = payload.audio_seconds;
        const wait = payload.wait_seconds;
        if (speech >= 5)
          setResult({
            kind: isTooSlow(speech, wait) ? "slow" : "ok",
            speech,
            wait,
          });
        else
          setResult((r) =>
            r.kind === "ok" || r.kind === "slow" ? r : { kind: "short", wait },
          );
      },
    );
    return () => {
      clearTimeout(holdTimer.current);
      offState.then((off) => off());
      offDone.then((off) => off());
    };
  }, [setLessons, threshold]);

  // Before the model is ready the shortcut is off; say why when it's pressed
  // here.
  useShortcutPress(
    shortcut,
    os,
    threshold,
    {
      onDown: () => !ready && setResult({ kind: "notYet" }),
    },
    !ready,
  );

  // A switched-to model gets one line when it's ready: how to check its speed.
  useEffect(() => {
    if (switchedTo && switchedTo === model?.id && ready)
      setResult({ kind: "switchedReady" });
  }, [switchedTo, model?.id, ready]);

  /* ---------- what it says ---------- */

  const nf = (v: number, digits: number) =>
    new Intl.NumberFormat(i18n.language, {
      minimumFractionDigits: digits,
      maximumFractionDigits: digits,
    }).format(v);
  const seconds = (s: number) =>
    t("onboarding.model.seconds", { value: nf(s, s < 10 ? 1 : 0) });
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
  const stillDownloading = eta
    ? t("onboarding.try.downloadingEta", { name, time: eta })
    : t("onboarding.try.downloading", { name });

  const keys = (
    <Keycaps
      binding={shortcut}
      osType={os}
      pressed={keysDown}
      className="mb-[3px]"
    />
  );

  const steps: { label: React.ReactNode; done: boolean; shown: boolean }[] = [
    { label: t("onboarding.try.stepClick"), done: clicked, shown: true },
    {
      label: <Trans i18nKey="onboarding.try.stepTalk" components={{ keys }} />,
      done: !!mode,
      shown: true,
    },
    {
      label:
        mode === "tap" ? (
          <Trans i18nKey="onboarding.try.stepTapAgain" components={{ keys }} />
        ) : (
          t("onboarding.try.stepLetGo")
        ),
      done: finished,
      shown: !!mode,
    },
  ];
  const nowIndex = steps.findIndex((s) => s.shown && !s.done);

  const faster =
    result.kind === "slow" && model
      ? fasterOption(
          model,
          languages,
          models.tier,
          models.picks,
          models.offered,
        )
      : null;

  const resultLine = (() => {
    switch (result.kind) {
      case "notYet":
        return (
          <p className="m-0">
            {download.state === "checking"
              ? t("onboarding.try.checking", { name })
              : download.state === "failed"
                ? t("onboarding.try.downloadFailed", { name })
                : t("onboarding.try.notYet", { text: stillDownloading })}
          </p>
        );
      case "empty":
        return <p className="m-0">{t("onboarding.try.empty")}</p>;
      case "failed":
        return <p className="m-0">{t("onboarding.try.failed")}</p>;
      case "noteClosed":
        return (
          <p className="m-0">
            <Trans
              i18nKey="onboarding.try.noteClosed"
              components={{
                link: (
                  <TextLink
                    onClick={() => {
                      setResult({ kind: "none" });
                      openNote();
                    }}
                  />
                ),
              }}
            />
          </p>
        );
      case "nothingTyped":
        return (
          <p className="m-0">
            <Trans
              i18nKey="onboarding.try.nothingTyped"
              components={{ b: <b className="font-bold text-text" /> }}
            />
          </p>
        );
      case "short":
        return (
          <p className="m-0">
            {t("onboarding.try.short", { wait: seconds(result.wait) })}
          </p>
        );
      case "ok":
        return (
          <p className="m-0">
            <span className="me-[6px] font-bold text-moss">{"✓"}</span>
            {t("onboarding.try.ok", {
              count: Math.round(result.speech),
              wait: seconds(result.wait),
            })}
          </p>
        );
      case "slow":
        if (!faster)
          return (
            <p className="m-0">
              {t("onboarding.try.ok", {
                count: Math.round(result.speech),
                wait: seconds(result.wait),
              })}
            </p>
          );
        return (
          <div className="flex flex-col items-start gap-[10px] rounded-card border border-line-strong bg-wash px-[14px] py-[12px]">
            <div>
              <b className="font-bold text-text">
                {t("onboarding.try.slowTook", {
                  spoke: seconds(result.speech),
                  took: seconds(result.wait),
                })}
              </b>{" "}
              {t(
                faster.worse
                  ? "onboarding.try.slowWorse"
                  : "onboarding.try.slowFaster",
                {
                  name: faster.model.name,
                  wait: seconds(
                    faster.wait * (result.speech / DICTATION_SECONDS),
                  ),
                },
              )}
            </div>
            <Button
              variant="outline"
              size="md"
              onClick={() => {
                setSwitchedTo(faster.model.id);
                setResult({ kind: "switched" });
                onSwitch(faster.model.id);
              }}
            >
              {t("onboarding.try.switch", {
                size: formatModelSize(models.infoFor(faster.model)?.size_mb),
              })}
            </Button>
          </div>
        );
      case "switched":
        return <p className="m-0">{t("onboarding.try.switched", { name })}</p>;
      case "switchedReady":
        return (
          <p className="m-0">{t("onboarding.try.switchedReady", { name })}</p>
        );
      default:
        return null;
    }
  })();

  const tookOne = lessons.hold || lessons.tap;
  return (
    <OnboardingShell
      progress={progress}
      pill={pill}
      onBack={onBack}
      // Once the model is ready, someone whose dictation can't work here yet
      // can still finish setting up.
      hint={
        ready && !tookOne ? (
          <TextLink onClick={onFinish}>{t("onboarding.try.skip")}</TextLink>
        ) : (
          ""
        )
      }
      primary={
        <Button
          variant="primary"
          size="lg"
          disabled={!preview && !tookOne}
          onClick={onFinish}
        >
          {t("onboarding.try.finish")}
        </Button>
      }
    >
      <div className="flex h-full min-h-0 flex-col items-start justify-start ps-gutter pe-[270px] pt-[4px] text-start">
        <ScreenTitle task>{t("onboarding.try.title")}</ScreenTitle>
        <ScreenSub className="mt-[8px]!">
          {t("onboarding.try.subtitle")}
        </ScreenSub>
        <ol className="m-0 mt-[22px] flex w-full list-none flex-col gap-[4px] p-0">
          {steps.map((step, i) =>
            step.shown ? (
              <li
                key={i}
                className={`flex min-h-[40px] items-center gap-[12px] ${
                  step.done
                    ? "text-ink-2"
                    : i === nowIndex
                      ? "font-semibold text-text"
                      : "text-ink-3"
                }`}
              >
                <span
                  className={`grid size-[22px] shrink-0 place-items-center rounded-full text-label font-bold ${
                    step.done
                      ? "border border-moss bg-moss text-background"
                      : i === nowIndex
                        ? "border-[1.5px] border-plum-edge bg-background text-text"
                        : "border border-line bg-wash text-ink-3"
                  }`}
                >
                  {step.done ? "✓" : i + 1}
                </span>
                <span className="inline-flex flex-wrap items-center gap-[6px] leading-[1.4]">
                  {step.label}
                </span>
              </li>
            ) : null,
          )}
          {!ready && download.state === "failed" && (
            <li className="ms-[34px] mt-[8px] text-small leading-[1.45] text-ink-2">
              <span className="text-warning">
                {t("onboarding.try.downloadFailed", { name })}
              </span>{" "}
              <TextLink onClick={() => model && onSwitch(model.id)}>
                {t("onboarding.download.retry")}
              </TextLink>
            </li>
          )}
          {!ready && download.state !== "failed" && (
            <li className="ms-[34px] mt-[8px] text-small leading-[1.45] text-ink-2">
              <div className="mb-[8px] h-[6px] overflow-hidden rounded-full bg-line">
                <i
                  className="block h-full rounded-[inherit] bg-background-ui transition-[width] duration-[120ms] ease-linear"
                  style={{
                    width: `${((download.state === "downloading" ? download.progress : 1) * 100).toFixed(1)}%`,
                  }}
                />
              </div>
              <span className="tabular-nums">
                {download.state === "checking"
                  ? t("onboarding.try.checking", { name })
                  : stillDownloading}
              </span>{" "}
              {t("onboarding.try.worksOnceDone")}
            </li>
          )}
        </ol>
        <div
          role="status"
          aria-live="polite"
          className="mt-[18px] w-full text-small leading-[1.45] text-ink-2 text-pretty"
        >
          {resultLine}
        </div>
      </div>
    </OnboardingShell>
  );
};
