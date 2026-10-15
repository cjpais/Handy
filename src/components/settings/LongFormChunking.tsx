import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface LongFormChunkingProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const LongFormChunking: React.FC<LongFormChunkingProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const enabled = getSetting("long_form_chunking") ?? true;

  return (
    <ToggleSwitch
      checked={enabled}
      onChange={(enabled) => updateSetting("long_form_chunking", enabled)}
      isUpdating={isUpdating("long_form_chunking")}
      label={t("settings.advanced.longFormChunking.title")}
      description={t("settings.advanced.longFormChunking.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
    />
  );
};
