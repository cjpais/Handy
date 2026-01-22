import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface CustomModelsToggleProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const CustomModelsToggle: React.FC<CustomModelsToggleProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const customModelsEnabled = getSetting("custom_models_enabled") ?? false;

  return (
    <ToggleSwitch
      checked={customModelsEnabled}
      onChange={(enabled) => updateSetting("custom_models_enabled", enabled)}
      isUpdating={isUpdating("custom_models_enabled")}
      label={t("settings.debug.customModels.label")}
      description={t("settings.debug.customModels.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
    />
  );
};
