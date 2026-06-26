import type { AppSettings, PasteMethod, TypingTool } from "@/bindings";

const DIRECT_PASTE_METHOD: PasteMethod = "direct";
const AUTO_TYPING_TOOL: TypingTool = "auto";
const REMOTE_DESKTOP_TYPING_TOOL: TypingTool = "remote_desktop";
const REMOTE_DESKTOP_CLIPBOARD_PASTE_METHODS: PasteMethod[] = [
  "ctrl_v",
  "ctrl_shift_v",
  "shift_insert",
];

/**
 * Returns whether the current output settings can use the Wayland Remote
 * Desktop portal authorization.
 */
export function isRemoteDesktopAuthorizationRelevant(
  pasteMethod: PasteMethod,
  typingTool: TypingTool,
): boolean {
  if (REMOTE_DESKTOP_CLIPBOARD_PASTE_METHODS.includes(pasteMethod)) {
    return true;
  }

  return (
    pasteMethod === DIRECT_PASTE_METHOD &&
    (typingTool === AUTO_TYPING_TOOL ||
      typingTool === REMOTE_DESKTOP_TYPING_TOOL)
  );
}

/**
 * Returns whether the persisted settings can use the Wayland Remote Desktop
 * portal authorization.
 */
export function isRemoteDesktopAuthorizationRelevantForSettings(
  settings: AppSettings,
): boolean {
  const pasteMethod = settings.paste_method ?? DIRECT_PASTE_METHOD;
  const typingTool = settings.typing_tool ?? AUTO_TYPING_TOOL;

  return isRemoteDesktopAuthorizationRelevant(pasteMethod, typingTool);
}
