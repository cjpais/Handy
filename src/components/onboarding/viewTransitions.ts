import {
  prefersReducedMotion,
  type ViewTransition,
  type WithViewTransitions,
} from "@/lib/utils/motion";

/**
 * One kind of View Transition (Demo to Try it, Model's moves), one at a
 * time. `canMove` is false without View Transitions (older webviews) or with
 * reduced motion: then it just changes. `skip` ends the last move of this
 * kind; `start` runs the next, and `onDone` cleans up after it unless a
 * newer one took over first. Each kind keeps its own: one kind starting
 * never cuts off another's cleanup.
 */
export const viewTransitions = () => {
  let running: ViewTransition | null = null;
  return {
    canMove: () =>
      !!(document as WithViewTransitions).startViewTransition &&
      !prefersReducedMotion(),
    skip: () => running?.skipTransition(),
    start: (update: () => void, onDone: () => void) => {
      const doc = document as WithViewTransitions;
      if (!doc.startViewTransition) return update();
      const transition = doc.startViewTransition(update);
      running = transition;
      // Skipped by a newer transition: fine, the DOM is already updated.
      transition.ready.catch(() => {});
      transition.finished.finally(() => {
        if (running !== transition) return;
        running = null;
        onDone();
      });
    },
  };
};
