import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface UpdateChecksToggleProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const UpdateChecksToggle: React.FC<UpdateChecksToggleProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating, updateChecksLocked } =
    useSettings();
  const updateChecksEnabled = getSetting("update_checks_enabled") ?? true;

  return (
    <ToggleSwitch
      checked={updateChecksEnabled}
      onChange={(enabled) => updateSetting("update_checks_enabled", enabled)}
      isUpdating={isUpdating("update_checks_enabled")}
      disabled={updateChecksLocked ?? false}
      label={t("settings.debug.updateChecks.label")}
      description={
        updateChecksLocked
          ? t("settings.debug.updateChecks.lockedDescription")
          : t("settings.debug.updateChecks.description")
      }
      descriptionMode={descriptionMode}
      grouped={grouped}
    />
  );
};
