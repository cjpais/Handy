import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";
import { useOsType } from "../../hooks/useOsType";

interface AirPodsModeProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const AirPodsMode: React.FC<AirPodsModeProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const osType = useOsType();

    const enabled = getSetting("airpods_mode") ?? false;

    if (osType !== "macos") {
      return null;
    }

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(enabled) => updateSetting("airpods_mode", enabled)}
        isUpdating={isUpdating("airpods_mode")}
        label={t("settings.advanced.airpodsMode.label")}
        description={t("settings.advanced.airpodsMode.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);
