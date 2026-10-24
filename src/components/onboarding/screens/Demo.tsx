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
import { Keycaps } from "../../shared/Keycaps";
import TrayIcon from "../../icons/TrayIcon";
import { OnboardingShell, ScreenHeader, ScreenMain } from "../OnboardingShell";
import { useOnboardingStore } from "../onboardingStore";
import { demoLine } from "../data/samples";
import "./demo.css";

type Mode = "hold" | "tap";
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
 *   press    the keys go down; the pill starts listening, the pointer
 *            steps aside and "Hold (or Tap) your shortcut" is lit
 *   lift     (tap) the keys come up; the pill keeps listening
 *   release  the keys come up (hold) or are tapped again (tap):
 *            "Transcribing", and "…your words appear." is lit
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
      [2400, "press"], //   the keys held, Handy listens: 2s
      [4400, "release"],
      [5200, "land"], //    the words stay 2s
    ],
    ms: 7200,
    still: 3400,
  },
  tap: {
    script: [
      [0, "reset"],
      [300, "point"],
      [1200, "click"],
      [2400, "press"],
      [2580, "lift"], //    a tap; Handy keeps listening: 2.8s
      [5400, "press"],
      [5580, "release"], // the second tap
      [6180, "land"], //    the words stay 2s
    ],
    ms: 8180,
    still: 4000,
  },
};

/** Your own press, here: how long "Transcribing" shows before the words. */
const USER_TRANSCRIBE_MS = 600;
/** …and how long your words stay before the loop picks up again. */
const USER_LINGER_MS = 3600;
/** A tap of yours that nobody taps again: Handy stops for you. */
const TAP_TIMEOUT_MS = 12000;
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
  shortcut: string;
  keyDown: boolean;
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
  shortcut,
  keyDown,
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
    const screen = screenRef.current;
    const sc = screen?.getBoundingClientRect();
    const f = fieldRef.current?.getBoundingClientRect();
    if (!screen || !sc || !f || pointer === "gone") return;
    setAt(
      pointer === "field"
        ? {
            x: f.left - sc.left + Math.min(58, f.width * 0.3),
            y: f.top - sc.top + f.height / 2,
          }
        : { x: screen.offsetWidth * 0.8, y: screen.offsetHeight * 0.86 },
    );
  }, [pointer, mode, os]);
  const isMac = os === "macos";
  // Handy's own icon, in the menu bar or the tray, as the real one changes:
  // an ear while it listens, a brain while it transcribes.
  const tray = (
    <span className="grid">
      <TrayIcon
        state={
          pill === "rec"
            ? "recording"
            : pill === "work"
              ? "transcribing"
              : "idle"
        }
        size={15}
      />
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
              <i className="w-[27.5px]" />
              <i className="w-[20px]" />
              <i className="w-[22.5px]" />
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
              <span className="flex gap-[6px]">
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
          {/* The shortcut, as a screencast shows keys: in the corner, down
              while it's pressed. */}
          <div className="demo-keys">
            <Keycaps
              binding={shortcut}
              osType={os}
              size="screen"
              pressed={keyDown}
            />
          </div>
          <span
            className="demo-pointer"
            data-p={pointer}
            style={{ transform: `translate(${at.x}px, ${at.y}px)` }}
          >
            {clickKey > 0 && <i key={clickKey} className="demo-click" />}
            <svg width="14" height="20" viewBox="0 0 11 16" aria-hidden="true">
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
  say: string | null;
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
      {[t("onboarding.demo.pressKeys"), t("onboarding.demo.addKey")].map(
        (line) => (
          <span key={line} aria-hidden="true" className={`${cell} invisible`}>
            {line}
          </span>
        ),
      )}
      {say && (
        <span role="status" className={`${cell} font-[650] text-text`}>
          {say}
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
 * into the app, then the shortcut goes down in the screen's corner, as keys
 * do in a screencast; under the laptop, the shortcut itself, to change. Hold and tap take turns, each told by one sentence whose
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
  const [keyDown, setKeyDown] = useState(false);
  const [pillState, setPillState] = useState<PillState>("hidden");
  const [text, setText] = useState({
    value: "",
    lang: "en",
    landed: false,
    key: 0,
  });

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
        setKeyDown(false);
        setPointer("rest");
        setFocused(false);
      } else if (beat === "point") {
        setPointer("field");
      } else if (beat === "click") {
        setPointer("field");
        setFocused(true);
        if (animate) setClickKey((k) => k + 1);
      } else if (beat === "press") {
        setKeyDown(true);
        // Out of the way while Handy listens; the still keeps it, so the
        // one picture shows the click as well.
        setPointer(animate ? "gone" : "field");
        setPillState((p) => (p === "hidden" ? "rec" : p));
        setPhrase(1);
      } else if (beat === "lift") {
        setKeyDown(false);
      } else if (beat === "release") {
        setKeyDown(false);
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
    setKeyDown(false);
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
      setKeyDown(true);
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
      setKeyDown(true);
      user.current = "down2";
    }
  };

  const onUp = (kind: PressKind) => {
    if (suspended.current || !user.current) return;
    if (user.current === "down2") return finish("tap");
    if (user.current !== "down" && user.current !== "holding") return;
    if (kind === "hold") return finish("hold");
    // A tap: the keys come back up and it keeps listening until the next
    // tap.
    clear();
    setKeyDown(false);
    setChapter("tap");
    user.current = "toggled";
    later(() => user.current === "toggled" && finish("tap"), TAP_TIMEOUT_MS);
  };

  /* ---------- the shortcut: suspended globally, heard here ---------- */

  const suspendBindings = useSuspendedShortcuts(!preview);

  const capture = useShortcutCapture(
    "transcribe",
    os,
    () => {
      suspended.current = false;
      suspendBindings();
      play("hold");
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
    ? t(onlyModifiers ? "onboarding.demo.addKey" : "onboarding.demo.pressKeys")
    : null;

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
            // 8px, not 4: the bold lead-in would read as the title's next line.
            subClassName="mt-[8px]! grid items-center justify-items-center"
          />
        }
      >
        <DemoLaptop
          os={os}
          shortcut={shortcut}
          keyDown={keyDown}
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
        {/* Your shortcut, as a caption to the drawing: the keys centred
            under the laptop, what they are on their left, Change on their
            right. One button, so there's one thing to focus. Changing happens
            in place: the keys give way to a dashed slot that takes the new
            ones, and Cancel (or Escape) stops. The press itself is shown in
            the screen. */}
        <button
          type="button"
          // Named by what it shows: "Your shortcut is ⌥ Space, Change".
          onClick={(e) => {
            if (e.detail) e.currentTarget.blur();
            if (capture.capturing) capture.cancel();
            else startCapture();
          }}
          // The shortcut may use Space; pressing it here mustn't click.
          onKeyDown={(e) => e.key === " " && e.preventDefault()}
          onKeyUp={(e) => e.key === " " && e.preventDefault()}
          // Shrink-wrapped, the two 1fr sides size to the wider of the two,
          // so the keys sit on the laptop's centre in any language.
          className="group/keycaps mt-[26px] inline-grid cursor-pointer grid-cols-[1fr_auto_1fr] items-center gap-[14px] text-small focus-visible:outline-none"
        >
          <span className="justify-self-end whitespace-nowrap text-ink-3">
            {t("onboarding.demo.yourShortcut")}
          </span>
          {/* The resting keys hold the cell's width while new ones go in,
              so nothing beside them moves. */}
          <span className="grid rounded-[9px] ring-offset-[6px] ring-offset-background group-focus-visible/keycaps:ring-2 group-focus-visible/keycaps:ring-logo-primary">
            <Keycaps
              binding={shortcut}
              osType={os}
              lift
              className={`col-start-1 row-start-1 justify-self-center ${capture.capturing ? "invisible" : ""}`}
            />
            {capture.capturing &&
              (partialKeys.length ? (
                <Keycaps
                  binding={capture.partial}
                  osType={os}
                  partial
                  className="col-start-1 row-start-1 justify-self-center"
                />
              ) : (
                <span className="col-start-1 row-start-1 h-[30px] min-w-[88px] translate-y-[3px] animate-pulse rounded-[7px] border border-dashed border-plum-pick" />
              ))}
          </span>
          <span className="justify-self-start whitespace-nowrap text-ink-2 underline decoration-line underline-offset-[3px] transition-colors group-hover/keycaps:text-text group-hover/keycaps:decoration-current">
            {t(
              capture.capturing
                ? "onboarding.demo.cancel"
                : "onboarding.demo.change",
            )}
          </span>
        </button>
      </ScreenMain>
    </OnboardingShell>
  );
};
