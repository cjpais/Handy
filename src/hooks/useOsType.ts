import { type } from "@tauri-apps/plugin-os";
import { type OSType } from "../lib/utils/keyboard";

/**
 * Get the current OS type for keyboard handling.
 * This is a simple wrapper - type() is synchronous.
 */
export function useOsType(): OSType {
  const osType = type();
  // type() returns "macos" | "windows" | "linux" | "ios" | "android"
  // OSType expects "macos" | "windows" | "linux" | "unknown"
  if (osType === "macos" || osType === "windows" || osType === "linux") {
    return osType;
  }
  return "unknown";
}
