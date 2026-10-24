import { flushSync } from "react-dom";

/**
 * Demo to Try it, and back: one camera move. Try it's screen is Demo's
 * laptop screen, close up, so going in, the drawn screen grows into the real
 * one, its window into the Notes window, and the laptop's lid and base pass
 * out of frame around it. The title rides up into place and Handy's glove
 * steps in last, once you're close. Back plays it the other way. Without
 * View Transitions (older webviews) or with reduced motion it just changes.
 *
 * What moves is named in screenTransitions.css, only while html carries
 * data-sflow, so Model's own transitions never see these names.
 */
export type ScreenMove = "in" | "out";

type ViewTransition = {
  ready: Promise<void>;
  finished: Promise<void>;
  skipTransition: () => void;
};
type WithViewTransitions = Document & {
  startViewTransition?: (update: () => void) => ViewTransition;
};

let running: ViewTransition | null = null;

const rect = (selector: string) =>
  document.querySelector<HTMLElement>(selector)?.getBoundingClientRect();

/**
 * The laptop's push: the transform that puts its screen where Try it's
 * screen is, about the drawn screen's centre. Going in it's where the lid
 * ends up; going back, where it starts.
 */
const setPush = (
  laptop: DOMRect | undefined,
  drawn: DOMRect | undefined,
  real: DOMRect | undefined,
) => {
  const style = document.documentElement.style;
  if (!laptop || !drawn || !real || !drawn.width) return;
  const cx = drawn.left + drawn.width / 2;
  const cy = drawn.top + drawn.height / 2;
  style.setProperty("--sf-ox", `${cx - laptop.left}px`);
  style.setProperty("--sf-oy", `${cy - laptop.top}px`);
  style.setProperty("--sf-x", `${real.left + real.width / 2 - cx}px`);
  style.setProperty("--sf-y", `${real.top + real.height / 2 - cy}px`);
  style.setProperty("--sf-s", `${real.width / drawn.width}`);
};

export const moveScreens = (kind: ScreenMove, update: () => void) => {
  const doc = document as WithViewTransitions;
  const reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;
  if (!doc.startViewTransition || reduce) {
    update();
    return;
  }
  running?.skipTransition();
  const html = document.documentElement;
  // Before the old picture is taken, so it's taken with the names on.
  html.dataset.sflow = kind;
  const laptop = rect(".demo-laptop");
  const drawn = rect(".demo-screen");
  const real = rect(".try-screen");
  const transition = doc.startViewTransition(() => {
    flushSync(update);
    if (kind === "in") setPush(laptop, drawn, rect(".try-screen"));
    else setPush(rect(".demo-laptop"), rect(".demo-screen"), real);
  });
  running = transition;
  // Skipped by a newer transition: fine, the DOM is already updated.
  transition.ready.catch(() => {});
  transition.finished.finally(() => {
    if (running !== transition) return;
    running = null;
    delete html.dataset.sflow;
    for (const v of ["--sf-ox", "--sf-oy", "--sf-x", "--sf-y", "--sf-s"])
      html.style.removeProperty(v);
  });
};
