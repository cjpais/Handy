/**
 * App-wide style comparison (temporary).
 *
 * The app is being unified under one look, and the onboarding design (v2) is
 * a candidate. Shared components carry both: their classes are today's look,
 * and `v2:` classes (App.css) override them when `data-style="v2"` is set on
 * the document root. This toggle lets the two be compared across the whole
 * app until one is chosen; then the other set of classes is deleted.
 *
 * The choice lives in localStorage only, so it can be applied synchronously
 * before React mounts and never touches persisted settings.
 */

export type AppStyle = "current" | "v2";

export const STYLE_STORAGE_KEY = "handy.style";

/** Apply a style to the document root and remember it for the next launch. */
export const applyStyle = (style: AppStyle): void => {
  const root = document.documentElement;
  if (style === "v2") {
    root.dataset.style = "v2";
  } else {
    delete root.dataset.style;
  }
  try {
    localStorage.setItem(STYLE_STORAGE_KEY, style);
  } catch {
    // localStorage may be unavailable; the style then resets on relaunch.
  }
};

/** Read the last-applied style for synchronous boot-time application. */
export const getStoredStyle = (): AppStyle => {
  try {
    if (localStorage.getItem(STYLE_STORAGE_KEY) === "v2") return "v2";
  } catch {
    // ignore
  }
  return "current";
};
