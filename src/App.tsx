import { useEffect, useState, useRef } from "react";
import { toast, Toaster } from "sonner";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { platform } from "@tauri-apps/plugin-os";
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
} from "tauri-plugin-macos-permissions-api";
import { ModelStateEvent, RecordingErrorEvent } from "./lib/types/events";
import "./App.css";
import AccessibilityPermissions from "./components/AccessibilityPermissions";
import Footer from "./components/footer";
import Onboarding, { AccessibilityOnboarding } from "./components/onboarding";
import { Sidebar, SidebarSection, SECTIONS_CONFIG } from "./components/Sidebar";
import { useSettings } from "./hooks/useSettings";
import { useSettingsStore } from "./stores/settingsStore";
import { commands } from "@/bindings";
import { getLanguageDirection, initializeRTL } from "@/lib/utils/rtl";

type OnboardingStep = "accessibility" | "model" | "done";

const renderSettingsContent = (section: SidebarSection) => {
  const ActiveComponent =
    SECTIONS_CONFIG[section]?.component || SECTIONS_CONFIG.general.component;
  return <ActiveComponent />;
};

function App() {
  const { t, i18n } = useTranslation();
  const [onboardingStep, setOnboardingStep] = useState<OnboardingStep | null>(
    null,
  );
  // Track if this is a returning user who just needs to grant permissions
  // (vs a new user who needs full onboarding including model selection)
  const [isReturningUser, setIsReturningUser] = useState(false);
  const [currentSection, setCurrentSection] =
    useState<SidebarSection>("general");
  const { settings, updateSetting } = useSettings();
  const direction = getLanguageDirection(i18n.language);
  const refreshAudioDevices = useSettingsStore(
    (state) => state.refreshAudioDevices,
  );
  const refreshOutputDevices = useSettingsStore(
    (state) => state.refreshOutputDevices,
  );
  const hasCompletedPostOnboardingInit = useRef(false);
  const [isDragActive, setIsDragActive] = useState(false);
  const isProcessingDrop = useRef(false);

  useEffect(() => {
    checkOnboardingStatus();
  }, []);

  // Initialize RTL direction when language changes
  useEffect(() => {
    initializeRTL(i18n.language);
  }, [i18n.language]);

  // Initialize Enigo, shortcuts, and refresh audio devices when main app loads
  useEffect(() => {
    if (onboardingStep === "done" && !hasCompletedPostOnboardingInit.current) {
      hasCompletedPostOnboardingInit.current = true;
      Promise.all([
        commands.initializeEnigo(),
        commands.initializeShortcuts(),
      ]).catch((e) => {
        console.warn("Failed to initialize:", e);
      });
      refreshAudioDevices();
      refreshOutputDevices();
    }
  }, [onboardingStep, refreshAudioDevices, refreshOutputDevices]);

  // Handle keyboard shortcuts for debug mode toggle
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      // Check for Ctrl+Shift+D (Windows/Linux) or Cmd+Shift+D (macOS)
      const isDebugShortcut =
        event.shiftKey &&
        event.key.toLowerCase() === "d" &&
        (event.ctrlKey || event.metaKey);

      if (isDebugShortcut) {
        event.preventDefault();
        const currentDebugMode = settings?.debug_mode ?? false;
        updateSetting("debug_mode", !currentDebugMode);
      }
    };

    // Add event listener when component mounts
    document.addEventListener("keydown", handleKeyDown);

    // Cleanup event listener when component unmounts
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [settings?.debug_mode, updateSetting]);

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

  // Prevent browser default file-drop behavior (navigates to file on WebKitGTK)
  useEffect(() => {
    const preventDefaults = (e: DragEvent) => {
      e.preventDefault();
      e.stopPropagation();
    };
    document.addEventListener("dragover", preventDefaults);
    document.addEventListener("drop", preventDefaults);
    return () => {
      document.removeEventListener("dragover", preventDefaults);
      document.removeEventListener("drop", preventDefaults);
    };
  }, []);

  // Drag and drop support via Tauri window events
  useEffect(() => {
    if (onboardingStep !== "done") return;

    let unlisten: (() => void) | null = null;
    let cancelled = false;
    let lastDropTimestamp = 0;
    const DROP_DEBOUNCE_MS = 3000;

    const SUPPORTED_AUDIO_EXTENSIONS = [
      "mp3", "m4a", "wav", "ogg", "flac", "aac", "wma", "aiff",
    ];

    const isAudioFile = (filePath: string): boolean => {
      const lower = filePath.toLowerCase();
      return SUPPORTED_AUDIO_EXTENSIONS.some((ext) => lower.endsWith(`.${ext}`));
    };

    const setupDragDrop = async () => {
      try {
        const appWindow = getCurrentWindow();

        unlisten = await appWindow.onDragDropEvent(async (event) => {
          if (cancelled) return;

          if (event.payload.type === "enter") {
            setIsDragActive(true);
            return;
          }
          if (event.payload.type === "leave" || event.payload.type === "over") {
            if (event.payload.type === "leave") setIsDragActive(false);
            return;
          }
          if (event.payload.type !== "drop") return;

          const paths = event.payload.paths;
          const now = Date.now();
          console.debug("[DragDrop] Drop event received, paths:", paths);

          // Guard 1: ref-based processing flag
          if (isProcessingDrop.current) {
            console.debug("[DragDrop] Already processing, skipping");
            return;
          }

          // Guard 2: timestamp-based debounce
          if (now - lastDropTimestamp < DROP_DEBOUNCE_MS) {
            console.debug("[DragDrop] Debounced, skipping");
            return;
          }

          isProcessingDrop.current = true;
          lastDropTimestamp = now;
          setIsDragActive(false);

          try {
            if (!paths || paths.length === 0) {
              toast.error(t("settings.history.noFilesDropped"));
              return;
            }

            const audioPaths = paths.filter((path: string) => isAudioFile(path));
            const nonAudioPaths = paths.filter((path: string) => !isAudioFile(path));

            if (audioPaths.length === 0) {
              toast.error(t("settings.history.unsupportedFormat"));
              return;
            }

            if (nonAudioPaths.length > 0) {
              toast.warning(
                t("settings.history.ignoringNonAudio", {
                  count: nonAudioPaths.length,
                }),
              );
            }

            for (const filePath of audioPaths) {
              try {
                const fileName = filePath.split("/").pop() || filePath;
                toast.info(
                  t("settings.history.importingFile", { name: fileName }),
                );
                const result = await commands.importAudioFile(filePath);
                if (result.status === "ok") {
                  toast.success(
                    t("settings.history.importedFile", { name: fileName }),
                  );
                } else {
                  toast.error(
                    `${t("settings.history.importFailed")}: ${result.error}`,
                  );
                }
              } catch (error) {
                toast.error(`${t("settings.history.importFailed")}: ${error}`);
              }
            }
          } finally {
            isProcessingDrop.current = false;
          }
        });
      } catch (error) {
        console.error("Failed to set up drag-drop listeners:", error);
      }
    };

    setupDragDrop();

    return () => {
      cancelled = true;
      if (unlisten) {
        unlisten();
      }
    };
  }, [onboardingStep, t]);

  // Listen for model loading failures and show a toast
  useEffect(() => {
    const unlisten = listen<ModelStateEvent>("model-state-changed", (event) => {
      if (event.payload.event_type === "loading_failed") {
        toast.error(
          t("errors.modelLoadFailed", {
            model:
              event.payload.model_name || t("errors.modelLoadFailedUnknown"),
          }),
          {
            description: event.payload.error,
          },
        );
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [t]);

  const revealMainWindowForPermissions = async () => {
    try {
      await commands.showMainWindowCommand();
    } catch (e) {
      console.warn("Failed to show main window for permission onboarding:", e);
    }
  };

  const checkOnboardingStatus = async () => {
    try {
      // Check if they have any models available
      const result = await commands.hasAnyModelsAvailable();
      const hasModels = result.status === "ok" && result.data;
      const currentPlatform = platform();

      if (hasModels) {
        // Returning user - check if they need to grant permissions first
        setIsReturningUser(true);

        if (currentPlatform === "macos") {
          try {
            const [hasAccessibility, hasMicrophone] = await Promise.all([
              checkAccessibilityPermission(),
              checkMicrophonePermission(),
            ]);
            if (!hasAccessibility || !hasMicrophone) {
              await revealMainWindowForPermissions();
              setOnboardingStep("accessibility");
              return;
            }
          } catch (e) {
            console.warn("Failed to check macOS permissions:", e);
            // If we can't check, proceed to main app and let them fix it there
          }
        }

        if (currentPlatform === "windows") {
          try {
            const microphoneStatus =
              await commands.getWindowsMicrophonePermissionStatus();
            if (
              microphoneStatus.supported &&
              microphoneStatus.overall_access === "denied"
            ) {
              await revealMainWindowForPermissions();
              setOnboardingStep("accessibility");
              return;
            }
          } catch (e) {
            console.warn("Failed to check Windows microphone permissions:", e);
            // If we can't check, proceed to main app and let them fix it there
          }
        }

        setOnboardingStep("done");
      } else {
        // New user - start full onboarding
        setIsReturningUser(false);
        setOnboardingStep("accessibility");
      }
    } catch (error) {
      console.error("Failed to check onboarding status:", error);
      setOnboardingStep("accessibility");
    }
  };

  const handleAccessibilityComplete = () => {
    // Returning users already have models, skip to main app
    // New users need to select a model
    setOnboardingStep(isReturningUser ? "done" : "model");
  };

  const handleModelSelected = () => {
    // Transition to main app - user has started a download
    setOnboardingStep("done");
  };

  // Still checking onboarding status
  if (onboardingStep === null) {
    return null;
  }

  if (onboardingStep === "accessibility") {
    return <AccessibilityOnboarding onComplete={handleAccessibilityComplete} />;
  }

  if (onboardingStep === "model") {
    return <Onboarding onModelSelected={handleModelSelected} />;
  }

  return (
    <div
      dir={direction}
      className="h-screen flex flex-col select-none cursor-default"
    >
      {/* Drag & drop overlay */}
      {isDragActive && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center"
          style={{
            backgroundColor: "rgba(0, 0, 0, 0.5)",
            backdropFilter: "blur(2px)",
          }}
        >
          <div
            className="bg-background border-2 border-dashed border-logo-primary rounded-xl px-8 py-6 shadow-2xl"
            style={{
              transform: "scale(1.05)",
              transition: "transform 200ms ease-in-out",
            }}
          >
            <div className="flex flex-col items-center gap-3">
              <div className="w-12 h-12 rounded-full bg-logo-primary/10 flex items-center justify-center">
                <span className="text-2xl">📂</span>
              </div>
              <p className="text-lg font-semibold text-logo-primary">
                {t("settings.history.dropOverlayTitle")}
              </p>
              <p className="text-sm text-text/60">
                {t("settings.history.dropOverlaySubtitle")}
              </p>
            </div>
          </div>
        </div>
      )}
      <Toaster
        theme="system"
        toastOptions={{
          unstyled: true,
          classNames: {
            toast:
              "bg-background border border-mid-gray/20 rounded-lg shadow-lg px-4 py-3 flex items-center gap-3 text-sm",
            title: "font-medium",
            description: "text-mid-gray",
          },
        }}
      />
      {/* Main content area that takes remaining space */}
      <div className="flex-1 flex overflow-hidden">
        <Sidebar
          activeSection={currentSection}
          onSectionChange={setCurrentSection}
        />
        {/* Scrollable content area */}
        <div className="flex-1 flex flex-col overflow-hidden">
          <div className="flex-1 overflow-y-auto">
            <div className="flex flex-col items-center p-4 gap-4">
              <AccessibilityPermissions />
              {renderSettingsContent(currentSection)}
            </div>
          </div>
        </div>
      </div>
      {/* Fixed footer at bottom */}
      <Footer />
    </div>
  );
}

export default App;
