import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { platform } from "@tauri-apps/plugin-os";
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
} from "tauri-plugin-macos-permissions-api";
import { Settings2 } from "lucide-react";
import { Toaster } from "sonner";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { HistorySettings } from "@/components/settings";
import { MeetingsView } from "./MeetingsView";
import { getLanguageDirection, initializeRTL } from "@/lib/utils/rtl";

type PrimaryTab = "meetings" | "transcription";

const PRIMARY_TABS: Array<{
  id: PrimaryTab;
  labelKey: string;
}> = [
  { id: "meetings", labelKey: "sidebar.meetings" },
  {
    id: "transcription",
    labelKey: "settings.advanced.groups.transcription",
  },
];

function PrimaryApp() {
  const { t, i18n } = useTranslation();
  const [activeTab, setActiveTab] = useState<PrimaryTab>("meetings");
  const hasInitialized = useRef(false);
  const direction = getLanguageDirection(i18n.language);

  useEffect(() => {
    initializeRTL(i18n.language);
  }, [i18n.language]);

  useEffect(() => {
    const unlisten = listen("meeting-summary", () => {
      setActiveTab("meetings");
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    if (hasInitialized.current) {
      return;
    }

    hasInitialized.current = true;

    void (async () => {
      try {
        const hasModelsResult = await commands.hasAnyModelsAvailable();
        const hasModels =
          hasModelsResult.status === "ok" && hasModelsResult.data;

        if (!hasModels) {
          await commands.showMainWindowCommand();
          return;
        }

        const currentPlatform = platform();

        if (currentPlatform === "macos") {
          const [hasAccessibility, hasMicrophone] = await Promise.all([
            checkAccessibilityPermission(),
            checkMicrophonePermission(),
          ]);

          if (!hasAccessibility || !hasMicrophone) {
            await commands.showMainWindowCommand();
            return;
          }
        }

        if (currentPlatform === "windows") {
          const microphoneStatus =
            await commands.getWindowsMicrophonePermissionStatus();
          if (
            microphoneStatus.supported &&
            microphoneStatus.overall_access === "denied"
          ) {
            await commands.showMainWindowCommand();
            return;
          }
        }

        await Promise.all([
          commands.initializeEnigo(),
          commands.initializeShortcuts(),
        ]);
      } catch (error) {
        console.warn("Primary window initialization failed:", error);
      }
    })();
  }, []);

  const handleOpenSettings = async () => {
    try {
      await commands.showMainWindowCommand();
    } catch (error) {
      console.warn("Failed to open main window:", error);
    }
  };

  return (
    <div
      dir={direction}
      className="h-screen overflow-hidden bg-warm-bone text-charcoal"
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
      <div className="flex h-full flex-col">
        <header className="border-b border-stone-mist bg-orange-off-white/95 px-6 py-5 backdrop-blur">
          <div className="mx-auto flex w-full max-w-6xl items-center justify-between gap-4">
            <div className="space-y-1">
              {/* eslint-disable-next-line i18next/no-literal-string */}
              <p className="text-[11px] font-mono font-semibold uppercase tracking-[0.18em] text-bark-grey">
                Handy
              </p>
              <div className="flex items-center gap-2 rounded-[14px] bg-warm-bone p-1">
                {PRIMARY_TABS.map((tab) => {
                  const isActive = tab.id === activeTab;
                  return (
                    <button
                      key={tab.id}
                      type="button"
                      onClick={() => setActiveTab(tab.id)}
                      className={`rounded-[10px] px-4 py-2 text-sm font-semibold transition-colors ${
                        isActive
                          ? "bg-forest-green text-orange-off-white shadow-sm"
                          : "text-bark-grey hover:bg-orange-off-white hover:text-charcoal"
                      }`}
                    >
                      {t(tab.labelKey)}
                    </button>
                  );
                })}
              </div>
            </div>
            <button
              type="button"
              onClick={handleOpenSettings}
              className="inline-flex items-center gap-2 rounded-[12px] border border-stone-mist bg-warm-bone px-4 py-2 text-sm font-semibold text-charcoal transition-colors hover:border-forest-green/40 hover:text-forest-green"
            >
              <Settings2 className="h-4 w-4" />
              {t("tray.settings")}
            </button>
          </div>
        </header>

        <main className="flex-1 overflow-y-auto px-6 py-6">
          <div className="mx-auto w-full max-w-6xl">
            {activeTab === "meetings" ? <MeetingsView /> : <HistorySettings />}
          </div>
        </main>
      </div>
    </div>
  );
}

export default PrimaryApp;
