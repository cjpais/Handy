import React from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import type { OverlayPosition } from "@/bindings";

interface ShowOverlayProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const ShowOverlay: React.FC<ShowOverlayProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const selectedPosition = (getSetting("overlay_position") ||
      "bottom") as OverlayPosition;

    const overlayOptions = [
      { value: "none", label: t('settings.advanced.overlay.none') },
      { value: "bottom", label: t('settings.advanced.overlay.bottom') },
      { value: "top", label: t('settings.advanced.overlay.top') },
    ];

    return (
      <SettingContainer
        title={t('settings.advanced.overlay.title')}
        description={t('settings.advanced.overlay.description')}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="flex items-center space-x-1">
          <Dropdown
            options={overlayOptions}
            selectedValue={selectedPosition}
            onSelect={handleOverlayPositionChange}
            disabled={isUpdating("overlay_position")}
          />
        </div>
      </SettingContainer>
    );

    async function handleOverlayPositionChange(position: string) {
      await updateSetting("overlay_position", position as OverlayPosition);
    }
  },
);