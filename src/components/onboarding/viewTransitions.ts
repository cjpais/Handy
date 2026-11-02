import { type } from "@tauri-apps/plugin-os";
import {
  prefersReducedMotion,
  type ViewTransition,
  type WithViewTransitions,
} from "@/lib/utils/motion";

/**
 * One View Transition at a time per kind; `onDone` is skipped if a newer one took over.
 * Off on Linux: WebKitGTK segfaults entering accelerated compositing with the
 * DMABUF renderer disabled (main.rs).
 */
export const viewTransitions = () => {
  let running: ViewTransition | null = null;
  return {
    canMove: () =>
      type() !== "linux" &&
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
