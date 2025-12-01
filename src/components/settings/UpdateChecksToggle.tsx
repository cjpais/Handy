import React from "react";
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
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const updateChecksEnabled = getSetting("update_checks_enabled") ?? true;

  return (
    <ToggleSwitch
      checked={updateChecksEnabled}
      onChange={(enabled) => updateSetting("update_checks_enabled", enabled)}
      isUpdating={isUpdating("update_checks_enabled")}
      label="Check for Updates"
      description="Allow Handy to automatically check for updates and enable manual checks from the footer or tray menu."
      descriptionMode={descriptionMode}
      grouped={grouped}
    />
  );
};
