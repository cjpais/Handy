import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface TranscriptionFormattingProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const TranscriptionFormatting: React.FC<TranscriptionFormattingProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    return (
      <>
        <ToggleSwitch
          checked={getSetting("lowercase_first_letter") ?? true}
          onChange={(enabled) =>
            updateSetting("lowercase_first_letter", enabled)
          }
          isUpdating={isUpdating("lowercase_first_letter")}
          label={t("settings.debug.lowercaseFirstLetter.label")}
          description={t("settings.debug.lowercaseFirstLetter.description")}
          descriptionMode={descriptionMode}
          grouped={grouped}
        />
        <ToggleSwitch
          checked={getSetting("remove_trailing_period") ?? true}
          onChange={(enabled) =>
            updateSetting("remove_trailing_period", enabled)
          }
          isUpdating={isUpdating("remove_trailing_period")}
          label={t("settings.debug.removeTrailingPeriod.label")}
          description={t("settings.debug.removeTrailingPeriod.description")}
          descriptionMode={descriptionMode}
          grouped={grouped}
        />
      </>
    );
  });
