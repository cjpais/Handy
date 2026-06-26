import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { commands } from "@/bindings";
import type { PasteMethod, TypingTool } from "@/bindings";
import { useWayland } from "./useWayland";

const DIRECT_PASTE_METHOD: PasteMethod = "direct";
const AUTO_TYPING_TOOL: TypingTool = "auto";
const REMOTE_DESKTOP_TYPING_TOOL: TypingTool = "remote_desktop";

const isRemoteDesktopAuthorizationRelevant = (
  pasteMethod: PasteMethod,
  typingTool: TypingTool,
): boolean =>
  pasteMethod === DIRECT_PASTE_METHOD &&
  (typingTool === AUTO_TYPING_TOOL ||
    typingTool === REMOTE_DESKTOP_TYPING_TOOL);

/**
 * Tracks whether automatic typing needs and has the Wayland portal authorization.
 *
 * Inputs: the current paste method and typing tool settings.
 * Outputs: relevance, authorization state, and a setter for optimistic UI updates.
 * Side effects: queries the Tauri backend and subscribes to authorization change
 * events while the permission is relevant.
 */
export function useRemoteDesktopAuthorization(
  pasteMethod: PasteMethod,
  typingTool: TypingTool,
) {
  const isWayland = useWayland();
  const isRelevant =
    isWayland && isRemoteDesktopAuthorizationRelevant(pasteMethod, typingTool);
  const [isAuthorized, setIsAuthorized] = useState(false);

  useEffect(() => {
    if (!isRelevant) {
      setIsAuthorized(false);
      return;
    }

    let isMounted = true;
    let unlisten: (() => void) | null = null;

    commands
      .getRemoteDesktopAuthorization()
      .then((authorized) => {
        if (isMounted) setIsAuthorized(authorized);
      })
      .catch(() => {
        if (isMounted) setIsAuthorized(false);
      });

    listen<boolean>("remote-desktop-auth-changed", (event) => {
      if (isMounted) setIsAuthorized(Boolean(event.payload));
    }).then((stop) => {
      unlisten = stop;
    });

    return () => {
      isMounted = false;
      if (unlisten) unlisten();
    };
  }, [isRelevant]);

  return { isRelevant, isAuthorized, setIsAuthorized };
}
