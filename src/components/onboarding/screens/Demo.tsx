import React, {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { Trans, useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { useShortcutCapture } from "@/hooks/useShortcutCapture";
import { type OSType, spaceDoesNotClick } from "@/lib/utils/keyboard";
import { prefersReducedMotion } from "@/lib/utils/motion";
import { cjkSubsetFont } from "@/lib/utils/cjkFont";
import { Button } from "../../ui/Button";
import { CapturedKeys, captureHintKey, Keycaps } from "../../shared/Keycaps";
import { DesktopBar } from "../DesktopBar";
import { WindowBar } from "../WindowBar";
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
/** `lift` is a tap's first key-up, while Handy keeps listening. */
type Beat = "reset" | "point" | "click" | "press" | "lift" | "release" | "land";

/**
 * Each way's beats, in ms from its start. `ms` is its full length (then the
 * other way plays); `still` is the moment shown under reduced motion.
 */
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

/** Under reduced motion: each way's still picture, in turn. */
const STILL_MS = 5000;

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
  // On Linux the pointer stays put and only clicks: WebKitGTK renders the
  // glide and fade unevenly. It sits at the field's end, clear of the text.
  const settled = os === "linux";
  const place: Pointer = settled ? "field" : pointer;
  // Measured: the field's position varies by app and OS.
  const screenRef = useRef<HTMLDivElement>(null);
  const fieldRef = useRef<HTMLDivElement>(null);
  const [at, setAt] = useState({ x: 0, y: 0 });
  useLayoutEffect(() => {
    const screen = screenRef.current;
    const sc = screen?.getBoundingClientRect();
    const f = fieldRef.current?.getBoundingClientRect();
    if (!screen || !sc || !f || place === "gone") return;
    setAt(
      place === "field"
        ? {
            x: settled
              ? f.right - sc.left - 22
              : f.left - sc.left + Math.min(58, f.width * 0.3),
            y: f.top - sc.top + f.height / 2,
          }
        : { x: screen.offsetWidth * 0.8, y: screen.offsetHeight * 0.86 },
    );
  }, [place, settled, mode, os]);
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
          <DesktopBar
            os={os}
            size="demo"
            state={
              pill === "rec"
                ? "recording"
                : pill === "work"
                  ? "transcribing"
                  : "idle"
            }
          />
          <div
            className="demo-app"
            data-mode={mode}
            data-focused={focused || undefined}
          >
            <WindowBar
              size="sm"
              title={t(
                mode === "hold"
                  ? "onboarding.demo.apps.messages"
                  : "onboarding.demo.apps.notes",
              )}
            />
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
                  className={`${landed ? "demo-landed" : ""} ${cjkSubsetFont(text) ?? ""}`}
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
            data-p={place}
            data-settled={settled || undefined}
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
 * Each way's instructions, current phrase lit. All variants share one grid
 * cell so switching never shifts layout, even when a translation wraps.
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
  downloadFailed: boolean;
  /** The download's status, beside the button it's holding back. */
  download: React.ReactNode;
  onRetry: () => void;
  onBack: () => void;
  onContinue: () => void;
}

/** Screen 4: an animated demo of hold and tap while the model downloads. */
export const Demo: React.FC<DemoProps> = ({
  progress,
  os,
  ready,
  downloadFailed,
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
  const [still] = useState(prefersReducedMotion);

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
        // Hide while listening; the still frame keeps it to show the click.
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

  /* ---------- changing the shortcut ---------- */

  // Normally on since Permissions; this is for when that screen was skipped
  // (Windows and Linux, mostly), so Change has shortcuts to change.
  useEffect(() => {
    if (!preview) commands.initializeShortcuts().catch(console.error);
  }, [preview]);

  // The loop stops while new keys go in, and starts over after.
  const capture = useShortcutCapture("transcribe", os, {
    onDone: () => play("hold"),
    inert: preview,
    escapeCancels: true,
  });

  const startCapture = () => {
    clear();
    apply(chapter, "reset");
    capture.start().then((began) => began || play(chapter));
  };

  const say = capture.capturing ? t(captureHintKey(capture.partial)) : null;

  return (
    <OnboardingShell
      progress={progress}
      onBack={onBack}
      hint={download}
      primary={
        downloadFailed ? (
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
        {/* The shortcut and Change as one button; capture happens in place. */}
        <button
          type="button"
          // Named by what it shows: "Your shortcut is ⌥ Space, Change".
          onClick={(e) => {
            if (e.detail) e.currentTarget.blur();
            if (capture.capturing) capture.cancel();
            else startCapture();
          }}
          {...spaceDoesNotClick}
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
            {capture.capturing && (
              <CapturedKeys
                partial={capture.partial}
                osType={os}
                className="col-start-1 row-start-1"
                keysClassName="justify-self-center"
              />
            )}
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
