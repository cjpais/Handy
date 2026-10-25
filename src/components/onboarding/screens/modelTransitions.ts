import { flushSync } from "react-dom";
import { viewTransitions } from "../viewTransitions";

/**
 * Model screen view transitions. Each model keeps the same names in every state
 * (mf-<id>, plus -n/-s/-g/-m/-r/-d/-c for parts) so the browser animates it.
 */

const moves = viewTransitions();
let stagger: HTMLStyleElement | null = null;

/** Name what should move; returns the rows that got a name, in order. */
const nameAll = (root: HTMLElement): HTMLElement[] => {
  const rows: HTMLElement[] = [];
  for (const el of root.querySelectorAll<HTMLElement>("[data-vt]")) {
    const row = el.closest<HTMLElement>("[data-vt-row]");
    const scroll = el.closest<HTMLElement>("[data-model-scroll]");
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

export const moveModels = (root: HTMLElement | null, update: () => void) => {
  if (!root || !moves.canMove()) {
    update();
    return;
  }
  moves.skip();
  const before = new Set(nameAll(root).map((el) => el.dataset.vt));
  const html = document.documentElement;
  html.dataset.mflow = "";
  if (!stagger) {
    stagger = document.createElement("style");
    document.head.appendChild(stagger);
  }
  const sheet = stagger;
  moves.start(
    () => {
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
    },
    () => {
      delete html.dataset.mflow;
      sheet.textContent = "";
      const now = document.querySelector<HTMLElement>("[data-model-root]");
      if (now) clearNames(now);
    },
  );
};
