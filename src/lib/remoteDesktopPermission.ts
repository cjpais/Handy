import { platform } from "@tauri-apps/plugin-os";
import { commands } from "@/bindings";
import type { AppSettings } from "@/bindings";

interface RemoteDesktopPermissionState {
  isRelevant: boolean;
  isAuthorized: boolean;
}

const DIRECT_PASTE_METHOD = "direct";
const AUTO_TYPING_TOOL = "auto";
const REMOTE_DESKTOP_TYPING_TOOL = "remote_desktop";

const isRemoteDesktopAuthorizationRelevantSetting = (
  settings: AppSettings,
): boolean => {
  const pasteMethod = settings.paste_method ?? DIRECT_PASTE_METHOD;
  const typingTool = settings.typing_tool ?? AUTO_TYPING_TOOL;

  return (
    pasteMethod === DIRECT_PASTE_METHOD &&
    (typingTool === AUTO_TYPING_TOOL ||
      typingTool === REMOTE_DESKTOP_TYPING_TOOL)
  );
};

async function readRemoteDesktopAuthorizationRelevantSetting(): Promise<boolean> {
  const result = await commands.getAppSettings();

  if (result.status === "error") {
    return false;
  }

  return isRemoteDesktopAuthorizationRelevantSetting(result.data);
}

/**
 * Returns whether the Wayland Remote Desktop permission belongs in onboarding.
 *
 * Inputs: none.
 * Outputs: whether the permission is relevant for this platform/settings pair,
 * and whether the portal authorization is already active.
 * Side effects: reads app settings and portal authorization state from the
 * Tauri backend.
 */
export async function getRemoteDesktopPermissionState(): Promise<RemoteDesktopPermissionState> {
  if (platform() !== "linux") {
    return { isRelevant: false, isAuthorized: false };
  }

  const [isWayland, isRelevantSetting] = await Promise.all([
    commands.isWaylandActive(),
    readRemoteDesktopAuthorizationRelevantSetting(),
  ]);

  if (!isWayland || !isRelevantSetting) {
    return { isRelevant: false, isAuthorized: false };
  }

  const isAuthorized = await commands.getRemoteDesktopAuthorization();
  return { isRelevant: true, isAuthorized };
}
