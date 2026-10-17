import React, { useEffect, useState } from "react";
import { GlassPill, type GlassState } from "../overlay/GlassPill";
import { smoothLevels } from "../overlay/levels";
import bgBusy from "../../../docs/assets/desktop-busy.jpg";
import bgDark from "../../../docs/assets/desktop-dark.jpg";
import bgLight from "../../../docs/assets/desktop-light.jpg";
import "./preview.css";

type Backdrop = "dark" | "light" | "busy-dark" | "busy-light";

const BACKDROPS: Record<Backdrop, { image: string; theme: "dark" | "light" }> =
  {
    dark: { image: bgDark, theme: "dark" },
    light: { image: bgLight, theme: "light" },
    "busy-dark": { image: bgBusy, theme: "dark" },
    "busy-light": { image: bgBusy, theme: "light" },
  };

const LABELS = {
  ru: {
    recording: "Запись",
    arming: "Запуск микрофона",
    transcribing: "Распознавание",
    done: "Готово",
    cancel: "Отменить диктовку",
    error: "Не удалось вставить",
  },
  en: {
    recording: "Recording",
    arming: "Starting the microphone",
    transcribing: "Recognizing",
    done: "Done",
    cancel: "Cancel dictation",
    error: "Could not insert text",
  },
} as const;

/** A frozen, speech-like spectrum so screenshots are reproducible. */
const SPEECH = [
  0.62, 0.78, 0.9, 0.7, 0.55, 0.64, 0.4, 0.33, 0.28, 0.22, 0.18, 0.14, 0.1,
  0.08, 0.06, 0.04,
];
const QUIET = SPEECH.map((v) => v * 0.18);

interface StageSpec {
  id: string;
  backdrop: Backdrop;
  state: GlassState;
  levels: number[];
  elapsed: number;
}

const sequence = (
  backdrop: Backdrop,
  states: Array<[string, GlassState, number[], number]>,
): StageSpec[] =>
  states.map(([id, state, levels, elapsed]) => ({
    id: `${backdrop}-${id}`,
    backdrop,
    state,
    levels,
    elapsed,
  }));

export const STAGES: StageSpec[] = [
  ...sequence("dark", [
    ["arming", "arming", QUIET, 0],
    ["recording", "recording", SPEECH, 7],
    ["transcribing", "transcribing", QUIET, 12],
    ["done", "done", QUIET, 12],
    ["error", "error", QUIET, 12],
  ]),
  ...sequence("light", [
    ["arming", "arming", QUIET, 0],
    ["recording", "recording", SPEECH, 7],
    ["transcribing", "transcribing", QUIET, 12],
    ["done", "done", QUIET, 12],
    ["error", "error", QUIET, 12],
  ]),
  ...sequence("busy-dark", [
    ["recording", "recording", SPEECH, 7],
    ["error", "error", QUIET, 12],
  ]),
  ...sequence("busy-light", [
    ["recording", "recording", SPEECH, 7],
    ["error", "error", QUIET, 12],
  ]),
];

const Stage: React.FC<{ spec: StageSpec; lang: "ru" | "en" }> = ({
  spec,
  lang,
}) => {
  const { image, theme } = BACKDROPS[spec.backdrop];
  const labels = LABELS[lang];
  return (
    <figure
      className="pv-stage"
      data-testid={`stage-${spec.id}`}
      data-theme={theme}
      style={{ backgroundImage: `url(${image})` }}
    >
      <div className="pv-capsule">
        <GlassPill
          state={spec.state}
          levels={spec.levels}
          elapsedSeconds={spec.elapsed}
          errorText={labels.error}
          labels={labels}
        />
      </div>
      <figcaption className="pv-caption">{spec.id}</figcaption>
    </figure>
  );
};

/** One capsule fed by a simulated microphone, to watch the motion. */
const LiveStage: React.FC<{ lang: "ru" | "en" }> = ({ lang }) => {
  const [levels, setLevels] = useState<number[]>(QUIET);
  const [elapsed, setElapsed] = useState(0);
  const labels = LABELS[lang];

  useEffect(() => {
    let frame = 0;
    let smooth = QUIET;
    const tick = (t: number) => {
      const wave = Array.from({ length: 16 }, (_, i) => {
        const base = 0.55 + 0.4 * Math.sin(t / 220 + i * 0.7);
        const burst = Math.max(0, Math.sin(t / 900)) ** 2;
        return Math.max(0, Math.min(1, base * burst * (1 - i / 22)));
      });
      smooth = smoothLevels(smooth, wave, 0.3);
      setLevels(smooth);
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    const timer = setInterval(() => setElapsed((e) => e + 1), 1000);
    return () => {
      cancelAnimationFrame(frame);
      clearInterval(timer);
    };
  }, []);

  return (
    <figure
      className="pv-stage"
      data-theme="dark"
      style={{ backgroundImage: `url(${bgDark})` }}
    >
      <div className="pv-capsule">
        <GlassPill
          state="recording"
          levels={levels}
          elapsedSeconds={elapsed}
          labels={labels}
        />
      </div>
      <figcaption className="pv-caption">{"live"}</figcaption>
    </figure>
  );
};

export const OverlayGallery: React.FC<{
  lang: "ru" | "en";
  only?: string | null;
}> = ({ lang, only }) => {
  const stages = only ? STAGES.filter((s) => s.id === only) : STAGES;
  return (
    <div className="pv-gallery">
      {stages.map((spec) => (
        <Stage key={spec.id} spec={spec} lang={lang} />
      ))}
      {!only && <LiveStage lang={lang} />}
    </div>
  );
};
