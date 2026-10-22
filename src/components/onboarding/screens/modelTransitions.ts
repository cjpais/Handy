import { flushSync } from "react-dom";

/**
 * Model's motion: the same model carries the same view-transition name in
 * every state (mf-<id>, and -n name, -s size, -g grades, -m check, -r role,
 * -d words, -c codes), so the browser moves it from where it was to where it
 * is. "grow" / "shrink" open and fold the full list in place (the picks glide
 * into the table's first rows); "pick" swaps which card is the big one. Rows
 * that weren't on screen before rise in one after another. Without View
 * Transitions (older webviews) or with reduced motion it just changes.
 */
export type ModelMove = "grow" | "shrink" | "pick";

type ViewTransition = {
  ready: Promise<void>;
  finished: Promise<void>;
  skipTransition: () => void;
};
type WithViewTransitions = Document & {
  startViewTransition?: (update: () => void) => ViewTransition;
};

let running: ViewTransition | null = null;
let stagger: HTMLStyleElement | null = null;

/** Name what should move; returns the rows that got a name, in order. */
const nameAll = (root: HTMLElement): HTMLElement[] => {
  const rows: HTMLElement[] = [];
  for (const el of root.querySelectorAll<HTMLElement>("[data-vt]")) {
    const row = el.closest<HTMLElement>("[data-vt-row]");
    const scroll = el.closest<HTMLElement>(".model-flow-scroll");
    let ok = true;
    // In the list, only rows fully in view move; the rest are part of the page.
    if (row && scroll) {
      const s = scroll.getBoundingClientRect();
      const r = row.getBoundingClientRect();
      ok = r.top >= s.top - 1 && r.bottom <= s.bottom + 1;
    }
    el.style.viewTransitionName = ok ? (el.dataset.vt ?? "") : "none";
    if (ok && el.hasAttribute("data-vt-row")) rows.push(el);
  }
  return rows;
};

const clearNames = (root: HTMLElement) =>
  root
    .querySelectorAll<HTMLElement>("[data-vt]")
    .forEach((el) => (el.style.viewTransitionName = ""));

export const moveModels = (
  root: HTMLElement | null,
  kind: ModelMove,
  update: () => void,
) => {
  const doc = document as WithViewTransitions;
  const reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;
  if (!root || !doc.startViewTransition || reduce) {
    update();
    return;
  }
  running?.skipTransition();
  const before = new Set(nameAll(root).map((el) => el.dataset.vt));
  const html = document.documentElement;
  html.dataset.mflow = kind;
  if (!stagger) {
    stagger = document.createElement("style");
    document.head.appendChild(stagger);
  }
  const sheet = stagger;
  const transition = doc.startViewTransition(() => {
    flushSync(update);
    // The screen may have re-rendered into a new root; name what's there now.
    const now = document.querySelector<HTMLElement>("[data-model-root]");
    if (!now) return;
    const fresh = nameAll(now).filter((el) => !before.has(el.dataset.vt));
    sheet.textContent = fresh
      .slice(0, 14)
      .map((el, i) => {
        const parts = [el, ...el.querySelectorAll<HTMLElement>("[data-vt]")]
          .map((p) => `::view-transition-new(${p.dataset.vt})`)
          .join(",");
        return `${parts}{animation-delay:${60 + i * 18}ms}`;
      })
      .join("\n");
  });
  running = transition;
  // Skipped by a newer transition: fine, the DOM is already updated.
  transition.ready.catch(() => {});
  transition.finished.finally(() => {
    if (running !== transition) return;
    running = null;
    delete html.dataset.mflow;
    sheet.textContent = "";
    const now = document.querySelector<HTMLElement>("[data-model-root]");
    if (now) clearNames(now);
  });
};
