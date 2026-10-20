import React, { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "@/hooks/useSettings";
import { useShortcutPress, type PressKind } from "@/hooks/useShortcutPress";
import { useShortcutCapture } from "@/hooks/useShortcutCapture";
import { useSuspendedShortcuts } from "../useSuspendedShortcuts";
import { keycapParts, type OSType } from "@/lib/utils/keyboard";
import { Button } from "../../ui/Button";
import { TextLink } from "../../ui/TextLink";
import { Keycaps } from "../../shared/Keycaps";
import HandyHand from "../../icons/HandyHand";
import {
  OnboardingShell,
  ScreenMain,
  ScreenSub,
  ScreenTitle,
} from "../OnboardingShell";
import { useOnboardingStore } from "../onboardingStore";
import { demoLine } from "../data/samples";
import "./demo.css";

type Mode = "hold" | "tap";
type Hand = "idle" | "down" | "off";
type PillState = "hidden" | "rec" | "work";
type Beat = "reset" | "press" | "bounce" | "release" | "land";

/**
 * Hold and tap take turns, so someone who only watches sees both. Each has
 * one picture, the opposite of the other's: in hold the hand stays on the key
 * while it records; in tap it touches the key (450 ms, so it's visible) and
 * drops away, and recording carries on until the next tap.
 */
const CHAPTERS: Record<
  Mode,
  { script: [number, Beat][]; ms: number; still: number }
> = {
  hold: {
    script: [
      [0, "reset"],
      [900, "press"],
      [3700, "release"],
      [4300, "land"],
    ],
    ms: 7200,
    still: 2600,
  },
  tap: {
    script: [
      [0, "reset"],
      [900, "press"],
      [1350, "bounce"],
      [6000, "press"],
      [6450, "release"],
      [7050, "land"],
    ],
    ms: 9600,
    still: 3600,
  },
};

/** Nobody tapped again: stop for them, without the tick. */
const TAP_TIMEOUT_MS = 12000;

const reducedMotion = () =>
  matchMedia("(prefers-reduced-motion: reduce)").matches;

/* ---------------- the recording pill ---------------- */

const BARS = 9;
const STILL_BARS = [5, 9, 14, 18, 12, 16, 10, 7, 4];

const DemoPill: React.FC<{ state: PillState; still: boolean }> = ({
  state,
  still,
}) => {
  const { t } = useTranslation();
  const bars = useRef<(HTMLElement | null)[]>([]);
  useEffect(() => {
    const set = (h: (i: number) => number) =>
      bars.current.forEach((b, i) => b && (b.style.height = `${h(i)}px`));
    if (state !== "rec") return set(() => 3);
    if (still) return set((i) => STILL_BARS[i]);
    const t0 = performance.now();
    let raf = 0;
    const loop = (now: number) => {
      const s = (now - t0) / 1000;
      const syllable = Math.max(
        0,
        Math.sin(s * 9.1) * 0.5 +
          Math.sin(s * 3.7 + 1) * 0.35 +
          (Math.random() - 0.3) * 0.35,
      );
      const level = Math.min(1, 0.12 + syllable * 0.95);
      set((i) => {
        const shape = 0.35 + 0.65 * Math.sin(((i + 0.5) / BARS) * Math.PI);
        return Math.max(3, level * shape * (0.65 + Math.random() * 0.35) * 18);
      });
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, [state, still]);

  const close = (
    <span className="flex items-center justify-self-end">
      <span className="grid size-[22px] place-items-center rounded-full bg-line text-[14px] leading-none text-ink-3">
        ×
      </span>
    </span>
  );
  return (
    <div className="demo-pill" data-state={state} aria-hidden="true">
      <div className="relative h-[40px] flex-none">
        <span className="demo-pill-layer is-rec">
          <span className="flex ps-[5px]">
            <span className="demo-pill-dot" />
          </span>
          <span className="flex h-[18px] items-center gap-[3px] pe-[8px]">
            {Array.from({ length: BARS }, (_, i) => (
              <i
                key={i}
                ref={(el) => {
                  bars.current[i] = el;
                }}
                className="h-[3px] w-[4px] rounded-[2px] bg-logo-primary transition-[height] duration-[80ms] ease-linear"
              />
            ))}
          </span>
          {close}
        </span>
        <span className="demo-pill-layer is-work">
          <span className="flex ps-[5px]">
            <span className="demo-pill-spin" />
          </span>
          <span className="whitespace-nowrap px-[8px] text-ink-2">
            {t("overlay.transcribing")}
          </span>
          {close}
        </span>
      </div>
    </div>
  );
};

/* ---------------- the screen ---------------- */

const DemoScreen: React.FC<{
  os: OSType;
  app: string;
  text: string;
  landed: boolean;
  landKey: number;
  lang: string;
  pill: PillState;
  still: boolean;
  label: string;
}> = ({ os, app, text, landed, landKey, lang, pill, still, label }) => {
  const isMac = os === "macos";
  const tray = (
    <span
      className={`grid place-items-center transition-[color] duration-200 ${
        pill === "rec"
          ? "text-background-ui"
          : pill === "work"
            ? "opacity-45"
            : ""
      }`}
    >
      <HandyHand width={11} height={12} tone="current" />
    </span>
  );
  const clock = <span className="tabular-nums">9:41</span>;
  const barClass =
    "absolute inset-x-0 z-[2] flex items-center bg-background px-[10px] text-[10px] font-semibold text-ink-2";
  return (
    <div
      role="img"
      aria-label={label}
      className="relative h-[276px] w-full select-none overflow-hidden rounded-card border-[1.5px] border-line-strong bg-demo-screen"
    >
      {isMac ? (
        <div
          className={`${barClass} top-0 h-[20px] gap-[12px] border-b border-line`}
        >
          <span className="h-[10px] w-[9px] rounded-[45%_45%_50%_50%] bg-ink-2" />
          <i className="h-[5px] w-[30px] rounded-[3px] bg-line-strong" />
          <i className="h-[5px] w-[20px] rounded-[3px] bg-line-strong" />
          <i className="h-[5px] w-[24px] rounded-[3px] bg-line-strong" />
          <span className="flex-1" />
          {tray}
          {clock}
        </div>
      ) : (
        <div
          className={`${barClass} bottom-0 h-[26px] gap-[8px] border-t border-line`}
        >
          <span className="flex-1" />
          <span className="grid size-[12px] grid-cols-2 gap-[2px] opacity-80">
            {[0, 1, 2, 3].map((i) => (
              <i key={i} className="bg-ink-2" />
            ))}
          </span>
          {[0, 1, 2].map((i) => (
            <i key={i} className="size-[14px] rounded-[4px] bg-line-strong" />
          ))}
          <span className="flex-1" />
          {tray}
          {clock}
        </div>
      )}
      <div
        className={`absolute start-1/2 w-[460px] -ms-[230px] overflow-hidden rounded-[9px] border border-line-strong bg-demo-window text-start ${isMac ? "top-[38px]" : "top-[16px]"}`}
      >
        <div className="relative flex h-[22px] items-center border-b border-line px-[9px] text-[10.5px] font-semibold text-ink-3">
          {isMac ? (
            <>
              <span className="flex gap-[5px]">
                {[0, 1, 2].map((i) => (
                  <i
                    key={i}
                    className="size-[8px] rounded-full bg-line-strong"
                  />
                ))}
              </span>
              <span className="absolute inset-x-0 text-center">{app}</span>
            </>
          ) : (
            <>
              <span>{app}</span>
              <span className="ms-auto flex gap-[12px]">
                {[0, 1, 2].map((i) => (
                  <i key={i} className="h-[1.5px] w-[8px] bg-ink-3" />
                ))}
              </span>
            </>
          )}
        </div>
        <div className="min-h-[84px] px-[13px] py-[10px] text-[17px] leading-[1.4] text-text">
          <span
            key={landKey}
            dir="auto"
            lang={lang}
            className={landed ? "demo-landed rounded-[3px]" : "rounded-[3px]"}
          >
            {text}
          </span>
          <span className="demo-caret ms-[1px] inline-block h-[1.1em] w-[1.5px] bg-text align-[-0.18em]" />
        </div>
      </div>
      <div
        className={`absolute start-1/2 z-[3] -translate-x-1/2 rtl:translate-x-1/2 ${isMac ? "bottom-[12px]" : "bottom-[36px]"}`}
      >
        <DemoPill state={pill} still={still} />
      </div>
    </div>
  );
};

interface DemoProps {
  progress: { index: number; count: number };
  pill?: React.ReactNode;
  os: OSType;
  /** The model is ready, so Try it can start. */
  ready: boolean;
  /** Its download stopped: the pill offers a retry. */
  downloadFailed?: boolean;
  onBack: () => void;
  onContinue: () => void;
}

/**
 * Screen 4, while the model downloads: how Handy works. A drawn computer, and
 * under it the shortcut, which the logo hand presses; hold and tap take turns.
 * Pressing the real shortcut here stops the loop and plays the way you used
 * (Handy's rule: held 300 ms or longer is a hold). The global shortcut is
 * suspended while this screen is up, so pressing it doesn't record.
 */
export const Demo: React.FC<DemoProps> = ({
  progress,
  pill,
  os,
  ready,
  downloadFailed = false,
  onBack,
  onContinue,
}) => {
  const { t } = useTranslation();
  const { getSetting, resetBinding } = useSettings();
  const { languages, script, preview } = useOnboardingStore();
  const binding = getSetting("bindings")?.transcribe;
  const shortcut = binding?.current_binding ?? "";
  const threshold = getSetting("hold_threshold_ms") ?? 300;
  const [still] = useState(reducedMotion);

  const [chapter, setChapter] = useState<Mode>("hold");
  const [wayKey, setWayKey] = useState(0);
  const [hand, setHand] = useState<Hand>("idle");
  const [pillState, setPillState] = useState<PillState>("hidden");
  const [text, setText] = useState({
    value: "",
    lang: "en",
    landed: false,
    key: 0,
  });
  const [done, setDone] = useState({ hold: false, tap: false });

  // The engine's own bookkeeping, outside React's renders.
  const timers = useRef<ReturnType<typeof setTimeout>[]>([]);
  const nextMode = useRef<Mode>("hold");
  const user = useRef<null | "down" | "holding" | "toggled" | "down2" | "work">(
    null,
  );
  const suspended = useRef(false);

  const clear = () => {
    timers.current.forEach(clearTimeout);
    timers.current = [];
  };
  const later = (fn: () => void, ms: number) =>
    timers.current.push(setTimeout(fn, ms));

  const showChapter = (mode: Mode) => {
    setChapter(mode);
    setWayKey((k) => k + 1);
  };

  const apply = useCallback(
    (mode: Mode, beat: Beat, animate = true) => {
      if (beat === "reset") {
        showChapter(mode);
        setText((x) => ({ ...x, value: "", landed: false }));
        setPillState("hidden");
        setHand("idle");
      } else if (beat === "press") {
        setHand("down");
        setPillState((p) => (p === "hidden" ? "rec" : p));
      } else if (beat === "bounce") {
        setHand("off");
      } else if (beat === "release") {
        setHand("idle");
        setPillState("work");
      } else {
        const line = demoLine(languages, script, mode);
        setPillState("hidden");
        setText((x) => ({
          value: line.text,
          lang: line.language,
          landed: animate,
          key: x.key + 1,
        }));
      }
    },
    [languages, script],
  );

  const play = useCallback(() => {
    clear();
    user.current = null;
    const mode = nextMode.current;
    nextMode.current = mode === "hold" ? "tap" : "hold";
    const { script: beats, ms, still: at } = CHAPTERS[mode];
    if (still) {
      // Still frames: each way's picture, in turn.
      for (const [time, beat] of beats)
        if (time <= at) apply(mode, beat, false);
      later(play, 5000);
      return;
    }
    for (const [time, beat] of beats) later(() => apply(mode, beat), time);
    later(play, ms);
  }, [apply, still]);

  useEffect(() => {
    nextMode.current = "hold";
    play();
    return clear;
    // The loop starts once; it reschedules itself.
  }, []);

  /* ---------- their own keys ---------- */

  const finish = (mode: Mode, ticked = true) => {
    clear();
    setHand("idle");
    setPillState("work");
    user.current = "work";
    later(() => {
      apply(mode, "land");
      if (ticked) setDone((d) => ({ ...d, [mode]: true }));
      showChapter(mode);
      user.current = null;
      // then show them the other way
      nextMode.current = mode === "hold" ? "tap" : "hold";
      later(play, 3600);
    }, 600);
  };

  const onDown = () => {
    if (suspended.current || user.current === "work") return;
    if (!user.current) {
      clear();
      setText((x) => ({ ...x, value: "", landed: false }));
      setHand("down");
      setPillState("rec");
      user.current = "down";
      // Still down at the threshold: it's a hold.
      later(() => {
        if (user.current === "down") {
          user.current = "holding";
          showChapter("hold");
        }
      }, threshold);
    } else if (user.current === "toggled") {
      clear();
      setHand("down");
      user.current = "down2";
    }
  };

  const onUp = (kind: PressKind) => {
    if (suspended.current || !user.current) return;
    if (user.current === "down2") return finish("tap");
    if (user.current !== "down" && user.current !== "holding") return;
    if (kind === "hold") return finish("hold");
    // A tap: the hand comes off and it keeps listening until the next tap.
    clear();
    setHand("off");
    showChapter("tap");
    user.current = "toggled";
    later(
      () => user.current === "toggled" && finish("tap", false),
      TAP_TIMEOUT_MS,
    );
  };

  /* ---------- the shortcut: suspended globally, heard here ---------- */

  const suspendBindings = useSuspendedShortcuts(!preview);

  const capture = useShortcutCapture(
    "transcribe",
    os,
    () => {
      suspended.current = false;
      suspendBindings();
      play();
    },
    preview,
  );

  useShortcutPress(
    shortcut,
    os,
    threshold,
    { onDown, onUp },
    !capture.capturing,
  );

  // Pressing the keycaps works like the keys.
  const pressedAt = useRef<number | null>(null);
  const keyHandlers = {
    onPointerDown: (e: React.PointerEvent) => {
      e.preventDefault();
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
      pressedAt.current = performance.now();
      onDown();
    },
    onPointerUp: () => {
      if (pressedAt.current == null) return;
      const held = performance.now() - pressedAt.current;
      pressedAt.current = null;
      onUp(held >= threshold ? "hold" : "tap");
    },
  };

  const startCapture = () => {
    suspended.current = true;
    clear();
    user.current = null;
    apply(chapter, "reset");
    capture.start();
  };

  const [what, then] =
    chapter === "hold"
      ? [t("onboarding.demo.hold.what"), t("onboarding.demo.hold.then")]
      : [t("onboarding.demo.tap.what"), t("onboarding.demo.tap.then")];
  const defaultShortcut = binding?.default_binding ?? "";
  const shortcutLabel = (b: string) =>
    keycapParts(b, os)
      .map((p) => p.label)
      .join(os === "macos" ? " " : " + ");

  const handClass =
    hand === "down"
      ? "translate-y-[-2px] duration-[110ms]"
      : hand === "off"
        ? "translate-y-[15px] duration-200"
        : "translate-y-[4px] duration-200";

  return (
    <OnboardingShell
      progress={progress}
      pill={pill}
      onBack={onBack}
      hint={
        ready
          ? ""
          : t(
              downloadFailed
                ? "onboarding.demo.hintFailed"
                : "onboarding.demo.hint",
            )
      }
      primary={
        <Button
          variant="primary"
          size="lg"
          disabled={!ready}
          onClick={onContinue}
        >
          {t("onboarding.demo.tryIt")}
        </Button>
      }
    >
      <ScreenMain task className="gap-0">
        <ScreenTitle task>{t("onboarding.demo.title")}</ScreenTitle>
        <ScreenSub
          key={wayKey}
          className="demo-way-in mt-[8px]! min-h-[22px] max-w-none! text-center"
        >
          <span aria-live="polite">
            {done[chapter] && (
              <span
                aria-hidden="true"
                className="me-[7px] inline-grid size-[16px] place-items-center rounded-full bg-moss align-[-2px] after:-mt-[2px] after:h-[4px] after:w-[8px] after:-rotate-45 after:border-b-2 after:border-s-2 after:border-background after:content-['']"
              />
            )}
            <strong className="font-medium text-text">{what}</strong> {then}
          </span>
        </ScreenSub>
        <div className="mt-[18px] flex w-full flex-col items-center">
          <DemoScreen
            os={os}
            app={t(
              chapter === "hold"
                ? "onboarding.demo.apps.messages"
                : "onboarding.demo.apps.mail",
            )}
            text={text.value}
            landed={text.landed}
            landKey={text.key}
            lang={text.lang}
            pill={pillState}
            still={still}
            label={t("onboarding.demo.screenLabel")}
          />
          <p className="mt-[18px] mb-0 flex items-center justify-center gap-[10px] whitespace-nowrap pb-[26px] text-ink-2">
            <span>{t("onboarding.demo.yourShortcut")}</span>
            <span
              className="-translate-y-[2px]"
              {...(capture.capturing ? {} : keyHandlers)}
            >
              <Keycaps
                binding={
                  capture.capturing ? capture.partial || shortcut : shortcut
                }
                osType={os}
                size="demo"
                pressed={!capture.capturing && hand === "down"}
                partial={capture.capturing && !!capture.partial}
                className="cursor-pointer"
                lastKeyChildren={
                  capture.capturing ? null : (
                    <span
                      aria-hidden="true"
                      className={`pointer-events-none absolute start-1/2 top-full z-[2] -ms-[13px] text-hand transition-transform ease-glide ${handClass}`}
                    >
                      <HandyHand width={26} height={28} tone="current" />
                    </span>
                  )
                }
              />
            </span>
            <span className="inline-flex items-center gap-[14px] font-ui text-small font-medium">
              {capture.capturing ? (
                <>
                  <span role="status" className="font-semibold text-text">
                    {t("onboarding.demo.pressKeys")}
                  </span>
                  <TextLink onClick={capture.cancel}>
                    {t("onboarding.demo.cancel")}
                  </TextLink>
                </>
              ) : (
                <>
                  <TextLink onClick={startCapture}>
                    {t("onboarding.demo.change")}
                  </TextLink>
                  {defaultShortcut && shortcut !== defaultShortcut && (
                    <TextLink
                      onClick={async () => {
                        if (preview) return;
                        // Saving a binding registers it; keep it quiet here.
                        await resetBinding("transcribe");
                        suspendBindings();
                      }}
                    >
                      {t("onboarding.demo.useDefault", {
                        shortcut: shortcutLabel(defaultShortcut),
                      })}
                    </TextLink>
                  )}
                </>
              )}
            </span>
          </p>
        </div>
      </ScreenMain>
    </OnboardingShell>
  );
};
