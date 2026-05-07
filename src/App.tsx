import { useEffect, useRef, useState } from "react";
import { toast, Toaster } from "sonner";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { platform } from "@tauri-apps/plugin-os";
import { CircleDot, Sparkles } from "lucide-react";
import { ModelStateEvent, RecordingErrorEvent } from "./lib/types/events";
import "./App.css";
import AccessibilityPermissions from "./components/AccessibilityPermissions";
import Footer from "./components/footer";
import { Sidebar, SidebarSection, SECTIONS_CONFIG } from "./components/Sidebar";
import { useSettings } from "./hooks/useSettings";
import { commands } from "@/bindings";
import { getLanguageDirection, initializeRTL } from "@/lib/utils/rtl";

const renderSettingsContent = (section: SidebarSection) => {
  const ActiveComponent =
    SECTIONS_CONFIG[section]?.component || SECTIONS_CONFIG.general.component;
  return <ActiveComponent />;
};

function App() {
  const { t, i18n } = useTranslation();
  const [currentSection, setCurrentSection] =
    useState<SidebarSection>("general");
  const { settings, updateSetting, refreshAudioDevices, refreshOutputDevices } =
    useSettings();
  const direction = getLanguageDirection(i18n.language);
  const hasCompletedAppInit = useRef(false);
  const backend = settings?.transcription_backend ?? "live_stt";
  const contentMaxWidth =
    currentSection === "debug" ? "max-w-none" : "max-w-[1120px]";

  useEffect(() => {
    initializeRTL(i18n.language);
  }, [i18n.language]);

  useEffect(() => {
    if (!settings || hasCompletedAppInit.current) {
      return;
    }

    hasCompletedAppInit.current = true;
    Promise.all([
      commands.initializeEnigo(),
      commands.initializeShortcuts(),
      refreshAudioDevices(),
      refreshOutputDevices(),
    ]).catch((error) => {
      console.warn("Failed to initialize app shell:", error);
    });
  }, [refreshAudioDevices, refreshOutputDevices, settings]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
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

    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [settings?.debug_mode, updateSetting]);

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

  useEffect(() => {
    const unlisten = listen("paste-error", () => {
      toast.error(t("errors.pasteFailedTitle"), {
        description: t("errors.pasteFailed"),
      });
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [t]);

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

  return (
    <div
      dir={direction}
      className="h-screen cursor-default select-none bg-[radial-gradient(circle_at_top_left,_rgba(254,226,226,0.55),_transparent_28%),linear-gradient(180deg,_#f8fafc_0%,_#eef3f8_100%)]"
    >
      <Toaster
        theme="light"
        toastOptions={{
          unstyled: true,
          classNames: {
            toast:
              "rounded-2xl border border-slate-200 bg-white px-4 py-3 shadow-[0_20px_50px_rgba(15,23,42,0.12)] flex items-center gap-3 text-sm text-slate-800",
            title: "font-semibold text-slate-900",
            description: "text-slate-500",
          },
        }}
      />

      <div className="flex h-full flex-col p-5">
        <div className="flex min-h-0 flex-1 overflow-hidden rounded-[32px] border border-slate-200 bg-white/95 shadow-[0_24px_80px_rgba(15,23,42,0.08)] backdrop-blur-sm">
          <Sidebar
            activeSection={currentSection}
            onSectionChange={setCurrentSection}
          />

          <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
            <div className="border-b border-slate-200 px-8 py-6">
              <div className="flex flex-col gap-5 lg:flex-row lg:items-start lg:justify-between">
                <div className="max-w-3xl">
                  <h1 className="text-3xl font-semibold tracking-[-0.03em] text-slate-950">
                    {t("workspace.title")}
                  </h1>
                </div>

                <div className="flex flex-wrap items-center gap-2">
                  <span className="inline-flex items-center gap-2 rounded-full border border-red-200 bg-red-50 px-4 py-2 text-xs font-semibold text-red-700">
                    <Sparkles className="h-3.5 w-3.5" />
                    {t("workspace.liveSttDefault")}
                  </span>
                  <span className="inline-flex items-center gap-2 rounded-full border border-slate-200 bg-slate-50 px-4 py-2 text-xs font-semibold text-slate-700">
                    <CircleDot className="h-3.5 w-3.5" />
                    {backend === "live_stt"
                      ? t("workspace.liveStt")
                      : t("workspace.localModel")}
                  </span>
                </div>
              </div>
            </div>

            <div className="min-h-0 flex-1 overflow-y-auto">
              <div
                className={`mx-auto flex w-full ${contentMaxWidth} flex-col gap-5 p-6`}
              >
                <AccessibilityPermissions />
                {renderSettingsContent(currentSection)}
              </div>
            </div>
          </div>
        </div>

        <Footer />
      </div>
    </div>
  );
}

export default App;
