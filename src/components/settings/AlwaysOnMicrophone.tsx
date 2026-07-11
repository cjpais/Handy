import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface AlwaysOnMicrophoneProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const AlwaysOnMicrophone: React.FC<AlwaysOnMicrophoneProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const alwaysOnMode = getSetting("always_on_microphone") || false;

    return (
      <ToggleSwitch
        checked={alwaysOnMode}
        onChange={(enabled) => updateSetting("always_on_microphone", enabled)}
        isUpdating={isUpdating("always_on_microphone")}
        label={t("settings.debug.alwaysOnMicrophone.label")}
        description={t("settings.debug.alwaysOnMicrophone.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);
