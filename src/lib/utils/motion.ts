/** The person asked their system for less motion. */
export const prefersReducedMotion = (): boolean =>
  matchMedia("(prefers-reduced-motion: reduce)").matches;

/** A running View Transition. */
export type ViewTransition = {
  ready: Promise<void>;
  finished: Promise<void>;
  skipTransition: () => void;
};

/** `document`, with View Transitions where the webview has them. */
export type WithViewTransitions = Document & {
  startViewTransition?: (update: () => void) => ViewTransition;
};
