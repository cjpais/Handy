import React, {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { Trans, useTranslation } from "react-i18next";
import { useSettings } from "@/hooks/useSettings";
import { useShortcutPress, type PressKind } from "@/hooks/useShortcutPress";
import { useShortcutCapture } from "@/hooks/useShortcutCapture";
import { useSuspendedShortcuts } from "../useSuspendedShortcuts";
import { type OSType } from "@/lib/utils/keyboard";
import { Button } from "../../ui/Button";
import { TextLink } from "../../ui/TextLink";
import { Keycaps } from "../../shared/Keycaps";
import HandyHand from "../../icons/HandyHand";
import { OnboardingShell, ScreenHeader, ScreenMain } from "../OnboardingShell";
import { useOnboardingStore } from "../onboardingStore";
import { demoLine } from "../data/samples";
import "./demo.css";

type Mode = "hold" | "tap";
type Hand = "rest" | "down";
type PillState = "hidden" | "rec" | "work";
/** The pointer on the drawn screen: off to the side, on the field, gone. */
type Pointer = "rest" | "field" | "gone";
/** The sentence's lit phrase: click, press and talk, the words appear. */
type Phrase = 0 | 1 | 2;
type Beat = "reset" | "point" | "click" | "press" | "lift" | "release" | "land";

/* ------------------------------------------------------------------------
 * Timing. Everything the loop does, in ms from the start of each way.
 *
 *   reset    the app is empty; "Click where you'd type." is lit
 *   point    the pointer glides to the app's text field
 *   click    it clicks: the caret appears
 *   press    the hand presses the keys; the pill starts listening, the
 *            pointer steps aside and "Hold (or Tap) your shortcut" is lit
 *   lift     (tap) the hand comes off; the pill keeps listening
 *   release  the hand lets go (hold) or taps again (tap): "Transcribing",
 *            and "…your words appear." is lit
 *   land     the words appear in the app
 *
 * `ms` is the way's whole length; after it, the other way starts. `still` is
 * the moment drawn as a still picture when the system asks for reduced
 * motion.
 * ------------------------------------------------------------------------ */
const CHAPTERS: Record<
  Mode,
  { script: [number, Beat][]; ms: number; still: number }
> = {
  hold: {
    script: [
      [0, "reset"],
      [300, "point"], //    the glide takes 850ms
      [1200, "click"],
      [2400, "press"], //   the hand holds, Handy listens: 3.6s
      [6000, "release"],
      [6600, "land"], //    the words stay 2s
    ],
    ms: 8600,
    still: 4000,
  },
  tap: {
    script: [
      [0, "reset"],
      [300, "point"],
      [1200, "click"],
      [2400, "press"],
      [2580, "lift"], //    a tap; Handy keeps listening: 4.4s
      [7000, "press"],
      [7180, "release"], // the second tap
      [7780, "land"], //    the words stay 2s
    ],
    ms: 9780,
    still: 4000,
  },
};

/** Your own press, here: how long "Transcribing" shows before the words. */
const USER_TRANSCRIBE_MS = 600;
/** …and how long your words stay before the loop picks up again. */
const USER_LINGER_MS = 3600;
/** A tap of yours that nobody taps again: Handy stops for you. */
const TAP_TIMEOUT_MS = 12000;
/** "Saved" after a new shortcut, before the loop starts again. */
const SAVED_MS = 1500;
/** Under reduced motion: each way's still picture, in turn. */
const STILL_MS = 5000;

const MODIFIERS = [
  "ctrl",
  "control",
  "shift",
  "alt",
  "option",
  "meta",
  "command",
  "cmd",
  "super",
  "win",
  "windows",
  "fn",
];

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

/* ---------------- the laptop ---------------- */

const DemoLaptop: React.FC<{
  os: OSType;
  mode: Mode;
  text: string;
  landed: boolean;
  landKey: number;
  lang: string;
  pill: PillState;
  still: boolean;
  dimmed: boolean;
  pointer: Pointer;
  /** Bumped on each click, to replay its ring. */
  clickKey: number;
  /** The field has the caret: it's been clicked, or you pressed the keys. */
  focused: boolean;
}> = ({
  os,
  mode,
  text,
  landed,
  landKey,
  lang,
  pill,
  still,
  dimmed,
  pointer,
  clickKey,
  focused,
}) => {
  const { t } = useTranslation();
  // The pointer aims at the field, wherever this app and OS put it.
  const screenRef = useRef<HTMLDivElement>(null);
  const fieldRef = useRef<HTMLDivElement>(null);
  const [at, setAt] = useState({ x: 0, y: 0 });
  useLayoutEffect(() => {
    const sc = screenRef.current?.getBoundingClientRect();
    const f = fieldRef.current?.getBoundingClientRect();
    if (!sc || !f || pointer === "gone") return;
    setAt(
      pointer === "field"
        ? {
            x: f.left - sc.left + Math.min(46, f.width * 0.3),
            y: f.top - sc.top + f.height / 2,
          }
        : { x: sc.width * 0.8, y: sc.height * 0.86 },
    );
  }, [pointer, mode, os]);
  const isMac = os === "macos";
  // Handy's own icon, in the menu bar or the tray: pink while it listens.
  const tray = (
    <span
      className={`grid transition-[color] duration-200 ${pill === "rec" ? "text-background-ui" : ""}`}
    >
      <HandyHand width={10} height={11} tone="current" />
    </span>
  );
  const clock = <span className="tabular-nums">9:41</span>;
  return (
    <div
      role="img"
      aria-label={t("onboarding.demo.screenLabel")}
      className="demo-laptop"
    >
      <div className="demo-lid">
        <span className="demo-cam" />
        <div
          ref={screenRef}
          className={`demo-screen ${dimmed ? "is-dimmed" : ""}`}
        >
          {isMac ? (
            <div className="demo-bar is-menu">
              <span className="demo-apple" />
              <i className="w-[22px]" />
              <i className="w-[16px]" />
              <i className="w-[18px]" />
              <span className="flex-1" />
              {tray}
              {clock}
            </div>
          ) : (
            <div className="demo-bar is-task">
              <span className="flex-1" />
              <span className="demo-start">
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
            className="demo-app"
            data-mode={mode}
            data-focused={focused || undefined}
          >
            <div className="demo-app-bar">
              <span className="flex gap-[5px]">
                {[0, 1, 2].map((i) => (
                  <i key={i} />
                ))}
              </span>
              <span className="absolute inset-0 grid place-items-center">
                {t(
                  mode === "hold"
                    ? "onboarding.demo.apps.messages"
                    : "onboarding.demo.apps.notes",
                )}
              </span>
            </div>
            <div className="demo-app-body">
              {mode === "hold" ? (
                <p className="demo-them">{t("onboarding.demo.chat")}</p>
              ) : (
                <p className="demo-note-title">
                  {t("onboarding.demo.noteTitle")}
                </p>
              )}
              <div ref={fieldRef} className="demo-field">
                <span
                  key={landKey}
                  dir="auto"
                  lang={lang}
                  className={landed ? "demo-landed" : ""}
                >
                  {text}
                </span>
                <span className="demo-caret ms-[1px] inline-block h-[1.1em] w-[1.5px] bg-text align-[-0.18em]" />
              </div>
            </div>
          </div>
          <div className="demo-pill-wrap">
            <DemoPill state={pill} still={still} />
          </div>
          <span
            className="demo-pointer"
            data-p={pointer}
            style={{ transform: `translate(${at.x}px, ${at.y}px)` }}
          >
            {clickKey > 0 && <i key={clickKey} className="demo-click" />}
            <svg width="11" height="16" viewBox="0 0 11 16" aria-hidden="true">
              <path
                d="M1 1v12.2l3-2.9 1.9 4.5 2-.9-1.9-4.4h4.1z"
                fill="#111"
                stroke="#fff"
                strokeWidth="1.1"
                strokeLinejoin="round"
              />
            </svg>
          </span>
        </div>
      </div>
      <div className="demo-base">
        <span className="demo-notch" />
      </div>
      <div className="demo-floor" />
    </div>
  );
};

/* ---------------- the sentence ---------------- */

/** One phrase of the sentence, lit while it's happening. */
const Lit: React.FC<{ on?: boolean; children?: React.ReactNode }> = ({
  on = false,
  children,
}) => <span className={`demo-phrase ${on ? "is-on" : ""}`}>{children}</span>;

/**
 * What to do, said once per way: click, press and talk, let go (or tap
 * again). The phrase that's happening is lit. Both ways' sentences, and the
 * shortcut prompts that stand in for them while you change it, are stacked
 * in one cell, so switching never moves anything, even when a translation
 * wraps.
 */
const DemoSentence: React.FC<{
  mode: Mode;
  phrase: Phrase;
  /** While changing the shortcut, one line in the sentence's place. */
  say: { text: string; tone: "" | "ok" } | null;
}> = ({ mode, phrase, say }) => {
  const { t } = useTranslation();
  const cell = "col-start-1 row-start-1";
  return (
    <>
      {(["hold", "tap"] as const).map((m) => {
        const on = !say && m === mode;
        return (
          <span
            key={m}
            aria-hidden={!on}
            className={`${cell} ${on ? "demo-swap" : "invisible"}`}
          >
            <Trans
              i18nKey={`onboarding.demo.${m}`}
              components={[0, 1, 2].map((i) => (
                <Lit key={i} on={on && phrase === i} />
              ))}
            />
          </span>
        );
      })}
      {[
        t("onboarding.demo.pressKeys"),
        t("onboarding.demo.addKey"),
        t("onboarding.demo.saved"),
      ].map((line) => (
        <span key={line} aria-hidden="true" className={`${cell} invisible`}>
          {line}
        </span>
      ))}
      {say && (
        <span
          role="status"
          className={`${cell} font-[650] ${say.tone === "ok" ? "text-moss" : "text-text"}`}
        >
          {say.text}
        </span>
      )}
    </>
  );
};

interface DemoProps {
  progress: { index: number; count: number };
  os: OSType;
  /** The model is ready, so Try it can start. */
  ready: boolean;
  /** Its download stopped: the button retries it. */
  downloadFailed?: boolean;
  /** The download's status, beside the button it's holding back. */
  download?: React.ReactNode;
  onRetry?: () => void;
  onBack: () => void;
  onContinue: () => void;
}

/**
 * Screen 4, while the model downloads: how Handy works. A laptop, nearly all
 * screen, where a reply and a note get written by voice: the pointer clicks
 * into the app, then the logo hand presses the shortcut, drawn as large keys
 * under the laptop. Hold and tap take turns, each told by one sentence whose
 * phrases light up as they happen. It's a demo, not a lesson: Try it teaches
 * the way they actually press. Pressing the real shortcut here stops the
 * loop and plays the way you used (Handy's rule: held 300 ms or longer is a
 * hold). The global shortcut is suspended while this screen is up, so
 * pressing it doesn't record.
 */
export const Demo: React.FC<DemoProps> = ({
  progress,
  os,
  ready,
  downloadFailed = false,
  download,
  onRetry,
  onBack,
  onContinue,
}) => {
  const { t } = useTranslation();
  const { getSetting } = useSettings();
  const { languages, script, preview } = useOnboardingStore();
  const binding = getSetting("bindings")?.transcribe;
  const shortcut = binding?.current_binding ?? "";
  const threshold = getSetting("hold_threshold_ms") ?? 300;
  const [still] = useState(reducedMotion);

  const [chapter, setChapter] = useState<Mode>("hold");
  const [phrase, setPhrase] = useState<Phrase>(0);
  const [pointer, setPointer] = useState<Pointer>("rest");
  const [clickKey, setClickKey] = useState(0);
  const [focused, setFocused] = useState(false);
  const [hand, setHand] = useState<Hand>("rest");
  const [pillState, setPillState] = useState<PillState>("hidden");
  const [text, setText] = useState({
    value: "",
    lang: "en",
    landed: false,
    key: 0,
  });
  const [saved, setSaved] = useState(false);

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

  const apply = useCallback(
    (mode: Mode, beat: Beat, animate = true) => {
      if (beat === "reset") {
        setChapter(mode);
        setPhrase(0);
        setText((x) => ({ ...x, value: "", landed: false }));
        setPillState("hidden");
        setHand("rest");
        setPointer("rest");
        setFocused(false);
      } else if (beat === "point") {
        setPointer("field");
      } else if (beat === "click") {
        setPointer("field");
        setFocused(true);
        if (animate) setClickKey((k) => k + 1);
      } else if (beat === "press") {
        setHand("down");
        // Out of the way while Handy listens; the still keeps it, so the
        // one picture shows the click as well.
        setPointer(animate ? "gone" : "field");
        setPillState((p) => (p === "hidden" ? "rec" : p));
        setPhrase(1);
      } else if (beat === "lift") {
        setHand("rest");
      } else if (beat === "release") {
        setHand("rest");
        setPillState("work");
        setPhrase(2);
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

  const play = useCallback(
    (mode: Mode = nextMode.current) => {
      clear();
      user.current = null;
      nextMode.current = mode === "hold" ? "tap" : "hold";
      const { script: beats, ms, still: at } = CHAPTERS[mode];
      if (still) {
        for (const [time, beat] of beats)
          if (time <= at) apply(mode, beat, false);
        later(() => play(), STILL_MS);
        return;
      }
      for (const [time, beat] of beats) later(() => apply(mode, beat), time);
      later(() => play(), ms);
    },
    [apply, still],
  );

  useEffect(() => {
    nextMode.current = "hold";
    play();
    return clear;
    // The loop starts once; it reschedules itself.
  }, []);

  /* ---------- their own keys ---------- */

  const finish = (mode: Mode) => {
    clear();
    setHand("rest");
    setPillState("work");
    setPhrase(2);
    user.current = "work";
    later(() => {
      apply(mode, "land");
      user.current = null;
      later(() => play(mode === "hold" ? "tap" : "hold"), USER_LINGER_MS);
    }, USER_TRANSCRIBE_MS);
  };

  const onDown = () => {
    if (suspended.current || user.current === "work") return;
    if (!user.current) {
      clear();
      setText((x) => ({ ...x, value: "", landed: false }));
      setHand("down");
      setPillState("rec");
      setPointer("gone");
      setFocused(true);
      setPhrase(1);
      user.current = "down";
      // Still down at the threshold: it's a hold.
      later(() => {
        if (user.current === "down") {
          user.current = "holding";
          setChapter("hold");
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
    // A tap: the hand goes back to rest and it keeps listening until the
    // next tap.
    clear();
    setHand("rest");
    setChapter("tap");
    user.current = "toggled";
    later(() => user.current === "toggled" && finish("tap"), TAP_TIMEOUT_MS);
  };

  /* ---------- the shortcut: suspended globally, heard here ---------- */

  const suspendBindings = useSuspendedShortcuts(!preview);
  const shortcutNow = useRef(shortcut);
  shortcutNow.current = shortcut;
  const shortcutBefore = useRef(shortcut);

  const capture = useShortcutCapture(
    "transcribe",
    os,
    () => {
      suspended.current = false;
      suspendBindings();
      // Wait a tick for the new binding to arrive from the store.
      setTimeout(() => {
        if (shortcutNow.current !== shortcutBefore.current) {
          setSaved(true);
          later(() => {
            setSaved(false);
            play("hold");
          }, SAVED_MS);
        } else play("hold");
      });
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

  const startCapture = () => {
    suspended.current = true;
    shortcutBefore.current = shortcut;
    clear();
    user.current = null;
    apply(chapter, "reset");
    capture.start();
  };

  const partialKeys = capture.partial
    ? capture.partial.split("+").filter(Boolean)
    : [];
  const onlyModifiers =
    partialKeys.length > 0 &&
    partialKeys.every((k) => MODIFIERS.includes(k.toLowerCase()));
  const say = capture.capturing
    ? {
        text: t(
          onlyModifiers
            ? "onboarding.demo.addKey"
            : "onboarding.demo.pressKeys",
        ),
        tone: "" as const,
      }
    : saved
      ? { text: t("onboarding.demo.saved"), tone: "ok" as const }
      : null;

  // Resting, the fingertips sit just under the key; pressed, they meet its
  // bottom edge as the key drops its 3px. The hand rides inside the key, so
  // it moves on the key's own timing.
  const handClass =
    hand === "down" ? "-translate-y-[2px]" : "translate-y-[7px]";

  return (
    <OnboardingShell
      progress={progress}
      onBack={onBack}
      hint={
        download ??
        (ready
          ? ""
          : t(
              downloadFailed
                ? "onboarding.demo.hintFailed"
                : "onboarding.demo.hint",
            ))
      }
      primary={
        downloadFailed && onRetry ? (
          <Button variant="primary" size="lg" onClick={onRetry}>
            {t("onboarding.download.retryDownload")}
          </Button>
        ) : (
          <Button
            variant="primary"
            size="lg"
            disabled={!ready}
            onClick={onContinue}
          >
            {t("onboarding.demo.tryIt")}
          </Button>
        )
      }
    >
      <ScreenMain
        header={
          <ScreenHeader
            title={t("onboarding.demo.title")}
            sub={<DemoSentence mode={chapter} phrase={phrase} say={say} />}
            subClassName="grid items-center justify-items-center"
          />
        }
      >
        <DemoLaptop
          os={os}
          mode={chapter}
          text={text.value}
          landed={text.landed}
          landKey={text.key}
          lang={text.lang}
          pill={pillState}
          still={still}
          dimmed={capture.capturing}
          pointer={pointer}
          clickKey={clickKey}
          focused={focused}
        />
        {/* The keys, centred; Change hangs to their right. */}
        <div className="relative mt-[12px]">
          {capture.capturing ? (
            partialKeys.length ? (
              <Keycaps
                binding={capture.partial}
                osType={os}
                size="demo"
                partial
                className="-translate-y-[3px]"
              />
            ) : (
              <span
                aria-hidden="true"
                className="inline-block h-[36px] w-[112px] rounded-[9px] border border-dashed border-logo-primary align-middle"
              />
            )
          ) : (
            // The keys are the way to change them: they lift on hover, as
            // if you could pull them off and put others on.
            <button
              type="button"
              aria-label={t("onboarding.demo.changeShortcut")}
              onClick={(e) => {
                if (e.detail) e.currentTarget.blur();
                startCapture();
              }}
              // The shortcut may use Space; pressing it here mustn't click.
              onKeyDown={(e) => e.key === " " && e.preventDefault()}
              onKeyUp={(e) => e.key === " " && e.preventDefault()}
              className="group/keycaps peer block cursor-pointer rounded-[12px] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-logo-primary focus-visible:ring-offset-4 focus-visible:ring-offset-background"
            >
              <Keycaps
                binding={shortcut}
                osType={os}
                size="demo"
                pressed={hand === "down"}
                lift
                className="-translate-y-[3px]"
                lastKeyChildren={
                  <span
                    aria-hidden="true"
                    className={`pointer-events-none absolute start-1/2 top-full z-[2] -ms-[13px] text-plum transition-transform duration-[70ms] ease-out ${handClass}`}
                  >
                    <HandyHand width={26} height={28} tone="current" />
                  </span>
                }
              />
            </button>
          )}
          <TextLink
            onClick={capture.capturing ? capture.cancel : startCapture}
            className="absolute start-[calc(100%+14px)] top-1/2 -translate-y-1/2 whitespace-nowrap peer-hover:text-text peer-hover:decoration-current"
          >
            {t(
              capture.capturing
                ? "onboarding.demo.cancel"
                : "onboarding.demo.changeShortcut",
            )}
          </TextLink>
        </div>
      </ScreenMain>
    </OnboardingShell>
  );
};
