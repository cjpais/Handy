import { useCallback, useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { commands } from "@/bindings";

/**
 * Turns Handy's global shortcuts off while `active`, so a screen can listen
 * to the keys itself. Hiding Handy's window turns them back on (the backend
 * does it), so they go off again whenever the window is back in front.
 * Returns a way to turn them off again (after saving a new binding).
 */
export const useSuspendedShortcuts = (active: boolean) => {
  const suspend = useCallback(() => {
    if (!active) return;
    commands
      .initializeShortcuts()
      .then(() => commands.suspendAllBindings())
      .catch(console.error);
  }, [active]);

  useEffect(() => {
    if (!active) return;
    suspend();
    const unlisten = getCurrentWindow().onFocusChanged(({ payload }) => {
      if (payload) suspend();
    });
    return () => {
      unlisten.then((off) => off());
      commands.resumeAllBindings().catch(console.error);
    };
  }, [active, suspend]);

  return suspend;
};
