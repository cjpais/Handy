import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface DoubleTapActivationProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const DoubleTapActivation: React.FC<DoubleTapActivationProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const doubleTapEnabled = getSetting("double_tap_activation") || false;

    return (
      <ToggleSwitch
        checked={doubleTapEnabled}
        onChange={async (enabled) => {
          await updateSetting("double_tap_activation", enabled);
          if (enabled) {
            await updateSetting("push_to_talk", false);
          }
        }}
        isUpdating={isUpdating("double_tap_activation")}
        label={t("settings.general.doubleTapActivation.label")}
        description={t("settings.general.doubleTapActivation.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);
