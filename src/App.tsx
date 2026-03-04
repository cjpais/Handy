import { useCallback, useEffect, useState, useRef } from "react";
import { Toaster, toast } from "sonner";
import { useTranslation } from "react-i18next";
import { platform } from "@tauri-apps/plugin-os";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
} from "tauri-plugin-macos-permissions-api";
import "./App.css";
import AccessibilityPermissions from "./components/AccessibilityPermissions";
import Footer from "./components/footer";
import Onboarding, { AccessibilityOnboarding } from "./components/onboarding";
import { Sidebar, SidebarSection, SECTIONS_CONFIG } from "./components/Sidebar";
import { useSettings } from "./hooks/useSettings";
import { useSettingsStore } from "./stores/settingsStore";
import {
  useFileImportStore,
  STAGE_I18N_KEYS,
  type FileImportProgress,
} from "./stores/fileImportStore";
import { commands } from "@/bindings";
import { getLanguageDirection, initializeRTL } from "@/lib/utils/rtl";

type OnboardingStep = "accessibility" | "model" | "done";

const renderSettingsContent = (section: SidebarSection) => {
  const ActiveComponent =
    SECTIONS_CONFIG[section]?.component || SECTIONS_CONFIG.general.component;
  return <ActiveComponent />;
};

function App() {
  const { i18n, t } = useTranslation();
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
  const startFileImport = useFileImportStore((state) => state.start);
  const updateImportProgress = useFileImportStore(
    (state) => state.updateFromProgress,
  );
  const finishImportSuccess = useFileImportStore(
    (state) => state.finishSuccess,
  );
  const finishImportError = useFileImportStore((state) => state.finishError);
  const hasCompletedPostOnboardingInit = useRef(false);
  const progressToastIdRef = useRef<string | number | null>(null);
  const lastToastStageRef = useRef<string | null>(null);

  const importAudioFileFromDialog = useCallback(async () => {
    if (useFileImportStore.getState().isRunning) {
      toast.warning(t("toasts.fileImport.alreadyRunning"));
      return;
    }

    const selected = await open({
      multiple: false,
      filters: [{ name: "Audio", extensions: ["wav", "mp3", "m4a", "opus"] }],
    });

    if (!selected || Array.isArray(selected)) {
      return;
    }

    setCurrentSection("history");
    startFileImport(selected);
    progressToastIdRef.current = toast.loading(
      t("toasts.fileImport.preparing"),
    );
    lastToastStageRef.current = "starting";

    try {
      const response = await commands.transcribeAudioFile(selected);
      if (response.status === "error") {
        const message = response.error;
        if (useFileImportStore.getState().isRunning) {
          finishImportError(message);
          toast.error(message, {
            id: progressToastIdRef.current ?? undefined,
          });
          progressToastIdRef.current = null;
          lastToastStageRef.current = null;
        }
        return;
      }

      if (useFileImportStore.getState().isRunning) {
        // Fallback completion path when no done-progress event was delivered.
        finishImportSuccess();
        toast.success(t("toasts.fileImport.completed"), {
          id: progressToastIdRef.current ?? undefined,
        });
        progressToastIdRef.current = null;
        lastToastStageRef.current = null;
      }
    } catch (error) {
      const message =
        error instanceof Error
          ? error.message
          : t("toasts.fileImport.failedImport");
      if (useFileImportStore.getState().isRunning) {
        finishImportError(message);
        toast.error(message, {
          id: progressToastIdRef.current ?? undefined,
        });
        progressToastIdRef.current = null;
        lastToastStageRef.current = null;
      }
    }
  }, [
    finishImportError,
    finishImportSuccess,
    setCurrentSection,
    startFileImport,
    t,
  ]);

  useEffect(() => {
    checkOnboardingStatus();
    // Run only on initial mount.
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

  useEffect(() => {
    const unlistenProgress = listen<FileImportProgress>(
      "file-transcription-progress",
      (event) => {
        const payload = event.payload;
        updateImportProgress(payload);

        if (payload.done) {
          if (payload.stage === "failed") {
            toast.error(
              payload.message || t("toasts.fileImport.failedGeneric"),
              {
                id: progressToastIdRef.current ?? undefined,
              },
            );
          } else {
            toast.success(t("toasts.fileImport.completed"), {
              id: progressToastIdRef.current ?? undefined,
            });
          }
          progressToastIdRef.current = null;
          lastToastStageRef.current = null;
          return;
        }

        if (lastToastStageRef.current !== payload.stage) {
          const stageKey = STAGE_I18N_KEYS[payload.stage];
          const message = stageKey
            ? t(stageKey)
            : payload.message || t("toasts.fileImport.processing");
          toast.loading(message, {
            id: progressToastIdRef.current ?? undefined,
          });
          lastToastStageRef.current = payload.stage;
        }
      },
    );

    const unlistenPromise = listen("tray-import-audio-file", () => {
      importAudioFileFromDialog();
    });

    return () => {
      unlistenProgress.then((unlisten) => unlisten()).catch(() => {});
      unlistenPromise.then((unlisten) => unlisten()).catch(() => {});
    };
  }, [importAudioFileFromDialog, t, updateImportProgress]);

  const checkOnboardingStatus = async () => {
    try {
      // Check if they have any models available
      const result = await commands.hasAnyModelsAvailable();
      const hasModels = result.status === "ok" && result.data;

      if (hasModels) {
        // Returning user - but check if they need to grant permissions on macOS
        setIsReturningUser(true);
        if (platform() === "macos") {
          try {
            const [hasAccessibility, hasMicrophone] = await Promise.all([
              checkAccessibilityPermission(),
              checkMicrophonePermission(),
            ]);
            if (!hasAccessibility || !hasMicrophone) {
              // Missing permissions - show accessibility onboarding
              setOnboardingStep("accessibility");
              return;
            }
          } catch (e) {
            console.warn("Failed to check permissions:", e);
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
