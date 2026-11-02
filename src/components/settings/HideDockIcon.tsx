import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";
import { useOsType } from "../../hooks/useOsType";

interface HideDockIconProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const HideDockIcon: React.FC<HideDockIconProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const osType = useOsType();

    const hideDockIcon = getSetting("hide_dock_icon") ?? false;

    if (osType !== "macos") {
      return null;
    }

    return (
      <ToggleSwitch
        checked={hideDockIcon}
        onChange={(enabled) => updateSetting("hide_dock_icon", enabled)}
        isUpdating={isUpdating("hide_dock_icon")}
        label={t("settings.advanced.hideDockIcon.label")}
        description={t("settings.advanced.hideDockIcon.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
        tooltipPosition="bottom"
      />
    );
  },
);
