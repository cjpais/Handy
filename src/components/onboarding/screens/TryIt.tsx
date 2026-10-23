import React, { useEffect, useRef, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import { commands, events } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { useShortcutPress } from "@/hooks/useShortcutPress";
import type { OSType } from "@/lib/utils/keyboard";
import { formatModelSize } from "@/lib/utils/format";
import { Button } from "../../ui/Button";
import { TextLink } from "../../ui/TextLink";
import { Keycaps } from "../../shared/Keycaps";
import { ProgressLine } from "../../shared/ProgressLine";
import { OnboardingShell, ScreenHeader, ScreenMain } from "../OnboardingShell";
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
import "./tryit.css";

type Mode = "hold" | "tap";
type Result =
  | { kind: "none" }
  | { kind: "notYet" }
  | { kind: "nothingTyped" }
  /** The take went the other way; `live` while Handy still listens. */
  | { kind: "wrongWay"; live: boolean }
  | { kind: "empty" }
  | { kind: "failed" }
  | { kind: "short"; wait: number }
  | { kind: "ok"; speech: number; wait: number }
  | { kind: "slow"; speech: number; wait: number }
  | { kind: "switched" }
  | { kind: "switchedReady" };

const OTHER: Record<Mode, Mode> = { hold: "tap", tap: "hold" };
/** A lesson done: its line stays this long before the next lesson's app. */
const ADVANCE_MS = 2400;

/* ---------------- the apps ---------------- */

/**
 * The window on the desk: the lesson's app, inactive (grey controls, a light
 * shadow) until its field has the cursor, like a real window. A click
 * anywhere on it puts the cursor in the field.
 */
const AppWindow: React.FC<{
  os: OSType;
  title: string;
  active: boolean;
  children: React.ReactNode;
}> = ({ os, title, active, children }) => {
  const isMac = os === "macos";
  const name = (
    <span key={title} className="try-window-title">
      {title}
    </span>
  );
  return (
    <div
      className="try-window"
      data-active={active || undefined}
      onMouseDown={(e) => {
        const field = e.currentTarget.querySelector<HTMLTextAreaElement>(
          ".try-app:not([hidden]) textarea",
        );
        if (!field || e.target === field) return;
        e.preventDefault();
        field.focus();
      }}
    >
      <div className="try-window-bar" data-os={isMac ? "mac" : "win"}>
        {isMac ? (
          <>
            <span className="try-window-lights" aria-hidden="true">
              <i />
              <i />
              <i />
            </span>
            {name}
          </>
        ) : (
          <>
            {name}
            <span className="try-window-controls" aria-hidden="true">
              <svg viewBox="0 0 10 10">
                <path d="M0 5.5h10" />
              </svg>
              <svg viewBox="0 0 10 10">
                <rect x="0.5" y="0.5" width="9" height="9" rx="1" />
              </svg>
              <svg viewBox="0 0 10 10">
                <path d="M0.5 0.5l9 9M9.5 0.5l-9 9" />
              </svg>
            </span>
          </>
        )}
      </div>
      <div className="try-window-body">{children}</div>
    </div>
  );
};

/** The field's props: the cursor in it or out, as the lesson tracks it. */
type FieldProps = {
  onFocus: () => void;
  onBlur: () => void;
};

/**
 * Hold's app: Messages, as Demo drew it. A friend's question, and a reply
 * box that grows with what's said into it. Nothing is sent: the words stay
 * where Handy typed them, as they would in a real app.
 */
const Messages = React.forwardRef<HTMLTextAreaElement, FieldProps>(
  (field, ref) => {
    const { t } = useTranslation();
    // The box grows by a hidden copy of its text in the same grid cell.
    const [text, setText] = useState("");
    return (
      <div className="try-chat">
        <p className="try-them">{t("onboarding.demo.chat")}</p>
        <div className="try-reply" data-text={`${text} `}>
          <textarea
            ref={ref}
            rows={1}
            placeholder={t("onboarding.try.hold.placeholder")}
            aria-label={t("onboarding.try.hold.field")}
            spellCheck={false}
            onInput={(e) => setText(e.currentTarget.value)}
            {...field}
          />
        </div>
      </div>
    );
  },
);
Messages.displayName = "Messages";

/** Tap's app: a blank note, in a writing app's serif. */
const Notes = React.forwardRef<HTMLTextAreaElement, FieldProps>(
  (field, ref) => {
    const { t } = useTranslation();
    return (
      <textarea
        ref={ref}
        className="try-page"
        placeholder={t("onboarding.try.tap.placeholder")}
        aria-label={t("onboarding.try.tap.field")}
        spellCheck={false}
        {...field}
      />
    );
  },
);
Notes.displayName = "Notes";

/* ---------------- the screen ---------------- */

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
 * Screen 5: the first real dictation, both ways, one at a time. Hold
 * answers a friend in Messages; tap writes something longer in Notes. Beside
 * the steps, one window on a patch of desktop shows the lesson's app: click
 * into it, use the shortcut its way, and the words land at the cursor, as
 * they will in any app. A lesson done hands over to the next, and the window
 * turns into its app. A take the other way counts for nothing but a nudge,
 * so both are tried for real. Takes of 5 seconds or more are timed, and a
 * slow one offers a quicker model.
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

  // The ways this shortcut works: both, unless it's set to only one.
  const activation = getSetting("shortcut_activation");
  const ways: Mode[] =
    activation === "toggle"
      ? ["tap"]
      : activation === "push_to_talk"
        ? ["hold"]
        : ["hold", "tap"];
  const allDone = ways.every((w) => lessons[w]);

  const [lesson, setLesson] = useState<Mode>(
    () => ways.find((w) => !lessons[w]) ?? ways[0],
  );
  // This lesson's field has had the cursor.
  const [clicked, setClicked] = useState(false);
  // The way of the take in progress (or the last one).
  const [take, setTake] = useState<Mode | null>(null);
  const [keysDown, setKeysDown] = useState(false);
  const [result, setResult] = useState<Result>({ kind: "none" });
  const [switchedTo, setSwitchedTo] = useState<string | null>(null);
  // Which app's field has the cursor: its window is the active one.
  const [focused, setFocused] = useState<Mode | null>(null);
  const fields = {
    hold: useRef<HTMLTextAreaElement>(null),
    tap: useRef<HTMLTextAreaElement>(null),
  };

  // The engine's bookkeeping, for the event listeners.
  const lessonRef = useRef(lesson);
  lessonRef.current = lesson;
  const waysRef = useRef(ways);
  waysRef.current = ways;
  const takeRef = useRef<Mode | null>(null);
  const recordingRef = useRef(false);
  const lockedRef = useRef(false);
  const holdTimer = useRef<ReturnType<typeof setTimeout>>();
  const advanceTimer = useRef<ReturnType<typeof setTimeout>>();

  const name = model?.name ?? "";

  /** Bring a lesson's app forward. Not mid-take: the words would go nowhere. */
  const goTo = (next: Mode) => {
    clearTimeout(advanceTimer.current);
    if (recordingRef.current || next === lessonRef.current) return;
    // The app going behind lets go of the cursor, so nothing lands there.
    fields[lessonRef.current].current?.blur();
    lessonRef.current = next;
    setLesson(next);
    setClicked(false);
    setTake(null);
    takeRef.current = null;
    setResult((r) => (r.kind === "notYet" ? r : { kind: "none" }));
  };
  const goToRef = useRef(goTo);
  goToRef.current = goTo;

  /** The take's way, once it's known. The other way gets a nudge. */
  const setWay = (way: Mode | null) => {
    takeRef.current = way;
    setTake(way);
    if (way && way !== lessonRef.current)
      setResult({ kind: "wrongWay", live: true });
    else if (way)
      setResult((r) => (r.kind === "wrongWay" ? { kind: "none" } : r));
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
      const phase = payload.phase;
      if (phase === "started") {
        clearTimeout(advanceTimer.current);
        recordingRef.current = true;
        lockedRef.current = false;
        setKeysDown(true);
        clearTimeout(holdTimer.current);
        // Still going at the threshold without locking on: they're holding.
        holdTimer.current = setTimeout(() => {
          if (!lockedRef.current) setWay("hold");
        }, threshold);
      } else if (phase === "locked") {
        lockedRef.current = true;
        clearTimeout(holdTimer.current);
        setKeysDown(false);
        setWay("tap");
      } else {
        clearTimeout(holdTimer.current);
        setKeysDown(false);
        if (phase === "cancelled") {
          recordingRef.current = false;
          setWay(null);
        } else if (!takeRef.current) setWay(lockedRef.current ? "tap" : "hold");
      }
    });
    const offDone = events.transcriptionCompleteEvent.listen(({ payload }) => {
      recordingRef.current = false;
      const way = takeRef.current ?? "hold";
      const now = lessonRef.current;
      if (payload.outcome !== "typed") {
        setWay(null);
        setResult({ kind: payload.outcome });
        return;
      }
      // The words go wherever the cursor is: only into the app if it's
      // there, and Handy is the app in front.
      if (
        document.activeElement !== fields[now].current ||
        !document.hasFocus()
      ) {
        setWay(null);
        setResult({ kind: "nothingTyped" });
        return;
      }
      // The other way worked, but this lesson is for its own.
      if (way !== now) {
        takeRef.current = null;
        setTake(null);
        setResult({ kind: "wrongWay", live: false });
        return;
      }
      setLessons({ [now]: true });
      const speech = payload.audio_seconds;
      const wait = payload.wait_seconds;
      const slow = speech >= 5 && isTooSlow(speech, wait);
      if (speech >= 5) setResult({ kind: slow ? "slow" : "ok", speech, wait });
      else setResult({ kind: "short", wait });
      // On to the other way, unless it's done (or there's a quicker model
      // to read about first).
      const { lessons: done } = useOnboardingStore.getState();
      const next = waysRef.current.find((w) => w !== now && !done[w]);
      if (next && !slow)
        advanceTimer.current = setTimeout(
          () => goToRef.current(next),
          ADVANCE_MS,
        );
    });
    return () => {
      clearTimeout(holdTimer.current);
      clearTimeout(advanceTimer.current);
      offState.then((off) => off());
      offDone.then((off) => off());
    };
    // The refs carry what changes; `fields` holds the same two refs.
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

  // Every step of the lesson shows from the start, so nothing moves as
  // they're done.
  const done = lessons[lesson];
  const steps: { label: React.ReactNode; done: boolean }[] = [
    { label: t(`onboarding.try.${lesson}.stepClick`), done: clicked || done },
    {
      label: (
        <Trans
          i18nKey={`onboarding.try.${lesson}.stepTalk`}
          components={{ keys }}
        />
      ),
      done: take === lesson || done,
    },
    {
      label: (
        <Trans
          i18nKey={`onboarding.try.${lesson}.stepEnd`}
          components={{ keys }}
        />
      ),
      done,
    },
  ];
  const nowIndex = steps.findIndex((s) => !s.done);

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

  const bold = { b: <b className="font-bold text-text" /> };
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
      case "nothingTyped":
        return (
          <p className="m-0">
            <Trans
              i18nKey={`onboarding.try.${lesson}.nothingTyped`}
              components={bold}
            />
          </p>
        );
      case "wrongWay":
        return (
          <p className="m-0">
            <Trans
              i18nKey={`onboarding.try.${lesson}.${result.live ? "wrongWayLive" : "wrongWay"}`}
              components={bold}
            />
          </p>
        );
      case "short":
        return (
          <p className="m-0">
            <span className="me-[6px] font-bold text-moss">{"✓"}</span>
            {t(
              lesson === "hold"
                ? "onboarding.try.shortHold"
                : "onboarding.try.short",
              { wait: seconds(result.wait) },
            )}
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

  const fieldProps = (way: Mode): FieldProps => ({
    onFocus: () => {
      setFocused(way);
      if (way === lessonRef.current) setClicked(true);
    },
    onBlur: () => setFocused((f) => (f === way ? null : f)),
  });

  return (
    <OnboardingShell
      progress={progress}
      pill={pill}
      onBack={onBack}
      // Once the model is ready, someone whose dictation can't work here yet
      // can still finish setting up.
      hint={
        ready && !allDone ? (
          <TextLink onClick={onFinish}>{t("onboarding.try.skip")}</TextLink>
        ) : (
          ""
        )
      }
      primary={
        <Button
          variant="primary"
          size="lg"
          disabled={!preview && !allDone}
          onClick={onFinish}
        >
          {t("onboarding.try.finish")}
        </Button>
      }
    >
      {/* Split: the lessons, and beside them a desk with each one's app. */}
      <div className="grid h-full grid-cols-[minmax(0,1fr)_272px]">
        <ScreenMain
          align="start"
          wide
          className="pe-[20px]"
          header={
            <ScreenHeader
              title={t(
                ways.length > 1
                  ? "onboarding.try.title"
                  : "onboarding.try.titleOne",
              )}
              sub={t("onboarding.try.subtitle")}
            />
          }
        >
          {ways.length > 1 && (
            <div
              role="group"
              aria-label={t("onboarding.try.lessons")}
              className="try-lessons"
            >
              {ways.map((way, i) => (
                <button
                  key={way}
                  type="button"
                  className="try-lesson"
                  aria-pressed={way === lesson}
                  data-on={way === lesson || undefined}
                  data-done={lessons[way] || undefined}
                  // Choosing a lesson leaves the cursor where it is.
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={() => goTo(way)}
                >
                  <span className="try-lesson-mark">
                    {lessons[way] ? "✓" : i + 1}
                  </span>
                  <span className="try-lesson-name">
                    {t(`onboarding.try.${way}.name`)}
                  </span>
                </button>
              ))}
            </div>
          )}
          {/* Room for the fullest state (the steps and the faster-model
              card), so the centred screen doesn't shift as they appear. */}
          <div className="mt-[14px] flex min-h-[250px] w-full flex-col">
            <ol
              key={lesson}
              className="try-swap m-0 flex w-full list-none flex-col gap-[2px] p-0"
            >
              {steps.map((step, i) => (
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
                          ? "border-[1.5px] border-plum bg-background text-text"
                          : "border border-line bg-wash text-ink-3"
                    }`}
                  >
                    {step.done ? "✓" : i + 1}
                  </span>
                  <span className="inline-flex flex-wrap items-center gap-[6px] leading-[1.4]">
                    {step.label}
                  </span>
                </li>
              ))}
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
                  <ProgressLine
                    value={
                      download.state === "downloading" ? download.progress : 1
                    }
                    className="mb-[8px] h-[6px]"
                  />
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
              className="mt-[14px] w-full text-small leading-[1.45] text-ink-2 text-pretty"
            >
              {resultLine}
            </div>
          </div>
        </ScreenMain>
        <div className="try-desk">
          <AppWindow
            os={os}
            title={t(
              lesson === "hold"
                ? "onboarding.demo.apps.messages"
                : "onboarding.demo.apps.notes",
            )}
            active={focused === lesson}
          >
            {/* Both apps stay, so what's typed in one is still there on the
                way back; only the lesson's shows (and can take the cursor). */}
            {ways.map((way) => (
              <div key={way} hidden={way !== lesson} className="try-app">
                {way === "hold" ? (
                  <Messages ref={fields.hold} {...fieldProps("hold")} />
                ) : (
                  <Notes ref={fields.tap} {...fieldProps("tap")} />
                )}
              </div>
            ))}
          </AppWindow>
        </div>
      </div>
    </OnboardingShell>
  );
};
