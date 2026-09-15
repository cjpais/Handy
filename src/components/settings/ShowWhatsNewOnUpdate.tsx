import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface ShowWhatsNewOnUpdateProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const ShowWhatsNewOnUpdate: React.FC<ShowWhatsNewOnUpdateProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const enabled = getSetting("show_whats_new_on_update") ?? true;

  return (
    <ToggleSwitch
      checked={enabled}
      onChange={(nextEnabled) =>
        updateSetting("show_whats_new_on_update", nextEnabled)
      }
      isUpdating={isUpdating("show_whats_new_on_update")}
      label={t("settings.about.whatsNewUpdates.label")}
      description={t("settings.about.whatsNewUpdates.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
    />
  );
};
