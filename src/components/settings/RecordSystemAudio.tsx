import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface RecordSystemAudioProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const RecordSystemAudio: React.FC<RecordSystemAudioProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("record_system_audio") === true;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(val) => updateSetting("record_system_audio", val)}
        isUpdating={isUpdating("record_system_audio")}
        label={t("settings.sound.recordSystemAudio.title")}
        description={t("settings.sound.recordSystemAudio.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);
