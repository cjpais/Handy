import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getCurrentWindow } from "@tauri-apps/api/window";
import appIcon from "../../src-tauri/icons/32x32.png";

const APP_NAME = "Handy";

// The glyphs native caption buttons use, from Segoe Fluent Icons (Windows 11)
// or Segoe MDL2 Assets (Windows 10).
const GLYPH_MINIMIZE = "";
const GLYPH_MAXIMIZE = "";
const GLYPH_RESTORE = "";
const GLYPH_CLOSE = "";

const appWindow = getCurrentWindow();

const runWindowAction = (action: () => Promise<void>) => {
  action().catch((e) => console.warn("Window action failed:", e));
};

const CAPTION_BUTTON_CLASS =
  "flex h-full w-[46px] items-center justify-center [font-family:'Segoe_Fluent_Icons','Segoe_MDL2_Assets'] text-[10px] leading-none";

/**
 * Title bar for the undecorated main window on Windows (see lib.rs).
 *
 * Windows 10 paints a dark title bar pure black while the window is active.
 * Like VS Code and Chrome, this bar is instead only a shade darker than the
 * background while active, and matches the background while inactive.
 */
const TitleBar = () => {
  const { t } = useTranslation();
  // WebView2 holds keyboard focus whenever the window is active, so page focus
  // tracks window activation. Tauri's isFocused() can't be used for this: it
  // checks the parent window, which never has focus while the webview does.
  const [isActive, setIsActive] = useState(() => document.hasFocus());
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    const onFocus = () => setIsActive(true);
    const onBlur = () => setIsActive(false);
    window.addEventListener("focus", onFocus);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("blur", onBlur);
    };
  }, []);

  useEffect(() => {
    const syncMaximized = () => {
      appWindow
        .isMaximized()
        .then(setIsMaximized)
        .catch((e) => console.warn("Failed to read maximized state:", e));
    };
    syncMaximized();
    const unlisten = appWindow.onResized(syncMaximized);
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const maximizeLabel = isMaximized
    ? t("titleBar.restore")
    : t("titleBar.maximize");

  return (
    <div
      dir="ltr"
      data-tauri-drag-region="deep"
      data-active={isActive || undefined}
      className="flex h-(--titlebar-height) select-none items-center bg-background text-mid-gray data-active:bg-titlebar-active data-active:text-text"
    >
      <img src={appIcon} alt="" draggable={false} className="mx-2 size-4" />
      <span className="flex-1 truncate text-[12px]">{APP_NAME}</span>
      <button
        type="button"
        tabIndex={-1}
        aria-label={t("titleBar.minimize")}
        title={t("titleBar.minimize")}
        onClick={() => runWindowAction(() => appWindow.minimize())}
        className={`${CAPTION_BUTTON_CLASS} hover:bg-text/10 active:bg-text/20`}
      >
        {GLYPH_MINIMIZE}
      </button>
      <button
        type="button"
        tabIndex={-1}
        aria-label={maximizeLabel}
        title={maximizeLabel}
        onClick={() => runWindowAction(() => appWindow.toggleMaximize())}
        className={`${CAPTION_BUTTON_CLASS} hover:bg-text/10 active:bg-text/20`}
      >
        {isMaximized ? GLYPH_RESTORE : GLYPH_MAXIMIZE}
      </button>
      <button
        type="button"
        tabIndex={-1}
        aria-label={t("titleBar.close")}
        title={t("titleBar.close")}
        // Goes through CloseRequested, which hides the window to the tray.
        onClick={() => runWindowAction(() => appWindow.close())}
        className={`${CAPTION_BUTTON_CLASS} hover:bg-[#e81123] hover:text-white active:bg-[#f1707a] active:text-white`}
      >
        {GLYPH_CLOSE}
      </button>
    </div>
  );
};

export default TitleBar;
