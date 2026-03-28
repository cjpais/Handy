import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface PreviewBeforePasteProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const PreviewBeforePaste: React.FC<PreviewBeforePasteProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("preview_before_paste") ?? false;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(enabled) => updateSetting("preview_before_paste", enabled)}
        isUpdating={isUpdating("preview_before_paste")}
        label={t("settings.advanced.previewBeforePaste.label")}
        description={t("settings.advanced.previewBeforePaste.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  });
