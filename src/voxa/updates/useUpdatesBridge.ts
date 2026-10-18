import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { OPEN_UPDATES_EVENT, useUpdateStore } from "./updateStore";

/** DOM event the footer fires to ask the app to open the updates page. */
export const OPEN_UPDATES_DOM_EVENT = "voxa:open-updates";

/**
 * Starts following the backend update state and calls `onOpen` when the tray
 * menu or the footer asks for the page with the update card.
 */
export function useUpdatesBridge(onOpen: () => void): void {
  const init = useUpdateStore((s) => s.init);

  useEffect(() => {
    let stop: (() => void) | undefined;
    let cancelled = false;
    init()
      .then((unlisten) => {
        if (cancelled) unlisten();
        else stop = unlisten;
      })
      .catch((e) => console.warn("Failed to follow update state:", e));
    return () => {
      cancelled = true;
      stop?.();
    };
  }, [init]);

  useEffect(() => {
    const tray = listen(OPEN_UPDATES_EVENT, onOpen);
    window.addEventListener(OPEN_UPDATES_DOM_EVENT, onOpen);
    return () => {
      void tray.then((unlisten) => unlisten());
      window.removeEventListener(OPEN_UPDATES_DOM_EVENT, onOpen);
    };
  }, [onOpen]);
}
