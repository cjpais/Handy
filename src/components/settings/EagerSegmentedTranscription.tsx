import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface EagerSegmentedTranscriptionProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const EagerSegmentedTranscription: React.FC<EagerSegmentedTranscriptionProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("eager_segmented_transcription") ?? false;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(enabled) =>
          updateSetting("eager_segmented_transcription", enabled)
        }
        isUpdating={isUpdating("eager_segmented_transcription")}
        label={t("settings.advanced.eagerSegmentedTranscription.label")}
        description={t(
          "settings.advanced.eagerSegmentedTranscription.description",
        )}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  });
