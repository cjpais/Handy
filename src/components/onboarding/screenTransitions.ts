import { flushSync } from "react-dom";
import { viewTransitions } from "./viewTransitions";

/**
 * Demo ↔ Try it zoom: Demo's drawn screen scales into Try it's.
 * Transition names apply only while html has data-sflow (screenTransitions.css),
 * so Model's transitions never see them.
 */
export type ScreenMove = "in" | "out";

const moves = viewTransitions();

const rect = (selector: string) =>
  document.querySelector<HTMLElement>(selector)?.getBoundingClientRect();

/** Sets the transform that maps the drawn screen onto Try it's real one. */
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
  if (!moves.canMove()) {
    update();
    return;
  }
  moves.skip();
  const html = document.documentElement;
  // Before the old snapshot, so it's captured with the names applied.
  html.dataset.sflow = kind;
  const laptop = rect(".demo-laptop");
  const drawn = rect(".demo-screen");
  const real = rect(".try-screen");
  moves.start(
    () => {
      flushSync(update);
      if (kind === "in") setPush(laptop, drawn, rect(".try-screen"));
      else setPush(rect(".demo-laptop"), rect(".demo-screen"), real);
    },
    () => {
      delete html.dataset.sflow;
      for (const v of ["--sf-ox", "--sf-oy", "--sf-x", "--sf-y", "--sf-s"])
        html.style.removeProperty(v);
    },
  );
};
