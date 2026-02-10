import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface FilterSilenceProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const FilterSilence: React.FC<FilterSilenceProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("filter_silence") ?? true;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(enabled) => updateSetting("filter_silence", enabled)}
        isUpdating={isUpdating("filter_silence")}
        label={t("settings.debug.filterSilence.label")}
        description={t("settings.debug.filterSilence.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);
