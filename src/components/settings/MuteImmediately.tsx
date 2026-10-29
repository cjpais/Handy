import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface MuteImmediatelyToggleProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const MuteImmediately: React.FC<MuteImmediatelyToggleProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const muteWhileRecording = getSetting("mute_while_recording") ?? false;
    const muteImmediately = getSetting("mute_immediately") ?? false;

    return (
      <ToggleSwitch
        checked={muteImmediately}
        onChange={(enabled) => updateSetting("mute_immediately", enabled)}
        isUpdating={isUpdating("mute_immediately")}
        disabled={!muteWhileRecording}
        label={t("settings.debug.muteImmediately.label")}
        description={t("settings.debug.muteImmediately.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  });
