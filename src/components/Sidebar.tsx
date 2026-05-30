import React from "react";
import { useTranslation } from "react-i18next";
import { Cog, AudioLines } from "lucide-react";
import GoldfishTextLogo from "./icons/GoldfishTextLogo";

export type AppView = "main" | "settings";

interface SidebarProps {
  view: AppView;
  onSelectCapture: () => void;
  onOpenSettings: () => void;
}

export const Sidebar: React.FC<SidebarProps> = ({
  view,
  onSelectCapture,
  onOpenSettings,
}) => {
  const { t } = useTranslation();

  const isCaptureActive = view === "main";
  const isSettingsActive = view === "settings";

  return (
    <div className="flex flex-col w-40 items-center px-2">
      <GoldfishTextLogo width={120} className="m-4" />

      {/* Primary nav */}
      <div className="flex flex-col w-full items-center gap-1 pt-2 flex-1">
        <div
          className={`flex gap-2 items-center p-2 w-full rounded-lg cursor-pointer transition-colors ${
            isCaptureActive
              ? "bg-logo-primary/80"
              : "hover:bg-mid-gray/20 hover:opacity-100 opacity-85"
          }`}
          onClick={onSelectCapture}
        >
          <AudioLines width={24} height={24} className="shrink-0" />
          <p className="text-sm font-medium truncate">{t("sidebar.capture")}</p>
        </div>
      </div>

      {/* Settings control — aligned with bottom edge of the white inner panel */}
      <div
        className={`flex gap-2 items-center p-2 w-full rounded-lg cursor-pointer transition-colors mb-1 ${
          isSettingsActive
            ? "bg-logo-primary/80"
            : "hover:bg-mid-gray/20 hover:opacity-100 opacity-85"
        }`}
        onClick={onOpenSettings}
      >
        <Cog width={20} height={20} className="shrink-0" />
        <p className="text-sm font-medium truncate">{t("sidebar.settings")}</p>
      </div>
    </div>
  );
};
