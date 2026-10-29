import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { platform } from "@tauri-apps/plugin-os";
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
  requestAccessibilityPermission,
  requestMicrophonePermission,
} from "tauri-plugin-macos-permissions-api";
import { toast } from "sonner";
import { commands } from "@/bindings";

export type PermissionStatus = "checking" | "needed" | "waiting" | "granted";
export type PermissionPlatform = "macos" | "windows" | "other";

export interface Permissions {
  platform: PermissionPlatform;
  microphone: PermissionStatus;
  accessibility: PermissionStatus;
  /** The first check is done. */
  checked: boolean;
  allGranted: boolean;
  /** Asked once already: asking again opens the OS settings instead. */
  microphoneAsked: boolean;
  requestMicrophone: () => Promise<void>;
  requestAccessibility: () => Promise<void>;
}

const POLL_INTERVAL_MS = 1000;

const currentPlatform = (): PermissionPlatform => {
  const p = platform();
  return p === "macos" ? "macos" : p === "windows" ? "windows" : "other";
};

/** What the OS says right now: microphone (macOS, Windows) and accessibility (macOS). */
const readPermissions = async (
  os: PermissionPlatform,
): Promise<{ microphone: boolean; accessibility: boolean }> => {
  if (os === "macos") {
    const [accessibility, microphone] = await Promise.all([
      checkAccessibilityPermission(),
      checkMicrophonePermission(),
    ]);
    return { microphone, accessibility };
  }
  if (os === "windows") {
    const status = await commands.getWindowsMicrophonePermissionStatus();
    return {
      microphone: !status.supported || status.overall_access !== "denied",
      accessibility: true,
    };
  }
  return { microphone: true, accessibility: true };
};

/**
 * Microphone and accessibility permissions: check, request, then poll until
 * granted. Granting accessibility initializes typing and shortcuts, so no
 * relaunch is needed.
 */
export const usePermissions = (): Permissions => {
  const { t } = useTranslation();
  const [os] = useState(currentPlatform);
  const [microphone, setMicrophone] = useState<PermissionStatus>("checking");
  const [accessibility, setAccessibility] =
    useState<PermissionStatus>("checking");
  const [microphoneAsked, setMicrophoneAsked] = useState(false);
  const pollingRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const stopPolling = useCallback(() => {
    if (pollingRef.current) {
      clearInterval(pollingRef.current);
      pollingRef.current = null;
    }
  }, []);

  const apply = useCallback(
    (granted: { microphone: boolean; accessibility: boolean }) => {
      setMicrophone((prev) =>
        granted.microphone ? "granted" : prev === "waiting" ? prev : "needed",
      );
      setAccessibility((prev) => {
        if (granted.accessibility && prev !== "granted") {
          Promise.all([
            commands.initializeEnigo(),
            commands.initializeShortcuts(),
          ]).catch((e) => console.warn("Failed to initialize input:", e));
        }
        return granted.accessibility
          ? "granted"
          : prev === "waiting"
            ? prev
            : "needed";
      });
      return granted.microphone && granted.accessibility;
    },
    [],
  );

  const startPolling = useCallback(() => {
    if (pollingRef.current) return;
    pollingRef.current = setInterval(async () => {
      try {
        if (apply(await readPermissions(os))) stopPolling();
      } catch (error) {
        console.error("Error checking permissions:", error);
        stopPolling();
        toast.error(t("permissions.errors.checkFailed"));
      }
    }, POLL_INTERVAL_MS);
  }, [apply, os, stopPolling, t]);

  // The first check.
  useEffect(() => {
    readPermissions(os)
      .then(apply)
      .catch((error) => {
        console.error("Failed to check permissions:", error);
        toast.error(t("permissions.errors.checkFailed"));
        setMicrophone("needed");
        setAccessibility("needed");
      });
    return stopPolling;
    // The first check runs once.
  }, []);

  const requestAccessibility = async () => {
    try {
      await requestAccessibilityPermission();
      setAccessibility("waiting");
      startPolling();
    } catch (error) {
      console.error("Failed to request accessibility permission:", error);
      toast.error(t("permissions.errors.requestFailed"));
    }
  };

  const requestMicrophone = async () => {
    try {
      // macOS prompts once; after that, only System Settings can allow it.
      if (os === "windows" || microphoneAsked) {
        await commands.openMicrophonePrivacySettings();
      } else {
        await requestMicrophonePermission();
      }
      setMicrophoneAsked(true);
      setMicrophone("waiting");
      startPolling();
    } catch (error) {
      console.error("Failed to request microphone permission:", error);
      toast.error(t("permissions.errors.requestFailed"));
    }
  };

  return {
    platform: os,
    microphone,
    accessibility,
    checked: microphone !== "checking" && accessibility !== "checking",
    allGranted: microphone === "granted" && accessibility === "granted",
    microphoneAsked,
    requestMicrophone,
    requestAccessibility,
  };
};
