import React from "react";
import { useTranslation } from "react-i18next";
import { SettingContainer } from "../../ui/SettingContainer";

interface DebugPathsProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

const APP_DATA_PATH = "%APPDATA%/handy";
const MODELS_PATH = "%APPDATA%/handy/models";
const SETTINGS_PATH = "%APPDATA%/handy/settings_store.json";

export const DebugPaths: React.FC<DebugPathsProps> = ({
  descriptionMode = "inline",
  grouped = false,
}) => {
  const { t } = useTranslation();

  return (
    <SettingContainer
      title="Debug Paths"
      description="Display internal file paths and directories for debugging purposes"
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <div className="text-sm text-gray-600 space-y-2">
        <div>
          <span className="font-medium">
            {t("settings.debug.paths.appData")}
          </span>{" "}
          <span className="font-mono text-xs select-text">{APP_DATA_PATH}</span>
        </div>
        <div>
          <span className="font-medium">
            {t("settings.debug.paths.models")}
          </span>{" "}
          <span className="font-mono text-xs select-text">{MODELS_PATH}</span>
        </div>
        <div>
          <span className="font-medium">
            {t("settings.debug.paths.settings")}
          </span>{" "}
          <span className="font-mono text-xs select-text">{SETTINGS_PATH}</span>
        </div>
      </div>
    </SettingContainer>
  );
};
