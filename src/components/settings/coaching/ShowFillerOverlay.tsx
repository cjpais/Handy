import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../../ui/ToggleSwitch";
import { useSettings } from "../../../hooks/useSettings";

interface ShowFillerOverlayProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const ShowFillerOverlay: React.FC<ShowFillerOverlayProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("show_filler_overlay") ?? true;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(value) => updateSetting("show_filler_overlay", value)}
        isUpdating={isUpdating("show_filler_overlay")}
        label={t("coaching.showOverlay.label")}
        description={t("coaching.showOverlay.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);
