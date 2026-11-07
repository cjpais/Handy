import { platform } from "@tauri-apps/plugin-os";
import { commands } from "@/bindings";
import { isRemoteDesktopAuthorizationRelevantForSettings } from "./remoteDesktopAuthorization";

interface RemoteDesktopPermissionState {
  isRelevant: boolean;
  isAuthorized: boolean;
}

async function readRemoteDesktopAuthorizationRelevantSetting(): Promise<boolean> {
  const result = await commands.getAppSettings();

  if (result.status === "error") {
    return false;
  }

  return isRemoteDesktopAuthorizationRelevantForSettings(result.data);
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
