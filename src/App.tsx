import { useEffect, useRef } from "react";
import { toast, Toaster } from "sonner";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { platform } from "@tauri-apps/plugin-os";
import { ModelStateEvent, RecordingErrorEvent } from "./lib/types/events";
import "./App.css";
import SecureInputWarning from "./components/SecureInputWarning";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { ModelStatus } from "./components/ModelStatus";
import { PermissionsScreen } from "./components/Permissions";
import {
  AutostartToggle,
  MicrophoneSelector,
  ShortcutInput,
  ShowOverlay,
  StartHidden,
} from "./components/settings";
import { SettingsGroup } from "./components/ui/SettingsGroup";
import HandyTextLogo from "./components/icons/HandyTextLogo";
import { usePermissions } from "./hooks/usePermissions";
import { useSettings } from "./hooks/useSettings";
import { getLanguageDirection } from "@/lib/utils/rtl";
import { commands } from "@/bindings";

function App() {
  const { t, i18n } = useTranslation();
  const direction = getLanguageDirection(i18n.language);
  const { refreshAudioDevices } = useSettings();
  const permissions = usePermissions();
  const hasInitialized = useRef(false);

  // The window may start hidden; when something needs the user, show it.
  const needsPermissions = permissions.checked && !permissions.allGranted;
  useEffect(() => {
    if (needsPermissions) {
      commands.showMainWindowCommand().catch((e) => {
        console.warn("Failed to show the window for permissions:", e);
      });
    }
  }, [needsPermissions]);

  // Once the permissions are in place: set up typing and shortcuts, and list
  // the microphones (listing them earlier would trigger the OS prompt).
  useEffect(() => {
    if (permissions.allGranted && !hasInitialized.current) {
      hasInitialized.current = true;
      Promise.all([
        commands.initializeEnigo(),
        commands.initializeShortcuts(),
      ]).catch((e) => {
        console.warn("Failed to initialize:", e);
      });
      refreshAudioDevices();
    }
  }, [permissions.allGranted, refreshAudioDevices]);

  // Listen for recording errors from the backend and show a toast
  useEffect(() => {
    const unlisten = listen<RecordingErrorEvent>("recording-error", (event) => {
      const { error_type, detail } = event.payload;

      if (error_type === "microphone_permission_denied") {
        const currentPlatform = platform();
        const platformKey = `errors.micPermissionDenied.${currentPlatform}`;
        const description = t(platformKey, {
          defaultValue: t("errors.micPermissionDenied.generic"),
        });
        toast.error(t("errors.micPermissionDeniedTitle"), { description });
      } else if (error_type === "no_input_device") {
        toast.error(t("errors.noInputDeviceTitle"), {
          description: t("errors.noInputDevice"),
        });
      } else {
        toast.error(
          t("errors.recordingFailed", { error: detail ?? "Unknown error" }),
        );
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [t]);

  // Listen for typing failures and show a toast. The technical detail is
  // logged to handy.log on the Rust side.
  useEffect(() => {
    const unlisten = listen("paste-error", () => {
      toast.error(t("errors.typeFailedTitle"), {
        description: t("errors.typeFailed"),
      });
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [t]);

  // Listen for transcription failures and show a toast.
  // The payload is the backend error message (also logged to handy.log).
  useEffect(() => {
    const unlisten = listen<string>("transcription-error", (event) => {
      toast.error(t("errors.transcriptionFailedTitle"), {
        description: event.payload,
      });
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [t]);

  // Listen for model loading and download failures and show a toast
  useEffect(() => {
    const unlisteners = [
      listen<ModelStateEvent>("model-state-changed", (event) => {
        if (event.payload.event_type === "loading_failed") {
          toast.error(t("errors.modelLoadFailed"), {
            description: event.payload.error,
          });
        }
      }),
      listen<string>("model-download-failed", (event) => {
        toast.error(t("errors.modelDownloadFailed"), {
          description: event.payload,
        });
      }),
    ];
    return () => {
      unlisteners.forEach((p) => p.then((fn) => fn()));
    };
  }, [t]);

  // Still checking permissions
  if (!permissions.checked) {
    return null;
  }

  return (
    <>
      <Toaster
        theme="system"
        toastOptions={{
          unstyled: true,
          classNames: {
            toast:
              "bg-background border border-line rounded-lg shadow-lg px-4 py-3 flex items-center gap-3 text-sm",
            title: "font-medium",
            description: "text-ink-3",
          },
        }}
      />
      {needsPermissions ? (
        <PermissionsScreen permissions={permissions} />
      ) : (
        <div
          dir={direction}
          className="h-screen overflow-y-auto select-none cursor-default"
        >
          <div className="max-w-3xl w-full mx-auto p-4 space-y-6">
            <HandyTextLogo width={120} className="mx-auto mt-2" />
            <SecureInputWarning />
            <ErrorBoundary context="Model">
              <ModelStatus />
            </ErrorBoundary>
            <SettingsGroup title={t("settings.microphone.title")}>
              <MicrophoneSelector descriptionMode="tooltip" grouped={true} />
            </SettingsGroup>
            <SettingsGroup title={t("settings.shortcut.title")}>
              <ShortcutInput shortcutId="transcribe" grouped={true} />
            </SettingsGroup>
            <SettingsGroup title={t("settings.basics.title")}>
              <ShowOverlay descriptionMode="tooltip" grouped={true} />
              <StartHidden descriptionMode="tooltip" grouped={true} />
              <AutostartToggle descriptionMode="tooltip" grouped={true} />
            </SettingsGroup>
          </div>
        </div>
      )}
    </>
  );
}

export default App;
