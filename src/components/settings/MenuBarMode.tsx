import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface MenuBarModeProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const MenuBarMode: React.FC<MenuBarModeProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const menuBarMode = getSetting("menu_bar_mode") ?? false;

    return (
      <ToggleSwitch
        checked={menuBarMode}
        onChange={(enabled) => updateSetting("menu_bar_mode", enabled)}
        isUpdating={isUpdating("menu_bar_mode")}
        label={t("settings.advanced.menuBarMode.label")}
        description={t("settings.advanced.menuBarMode.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
        tooltipPosition="bottom"
      />
    );
  },
);
