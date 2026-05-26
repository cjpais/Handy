import React from "react";
import { useTranslation } from "react-i18next";
import type { OverlayTheme as OverlayThemeValue } from "@/bindings";
import { useSettings } from "../../hooks/useSettings";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";

interface OverlayThemeProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const OverlayTheme: React.FC<OverlayThemeProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const selectedTheme = (getSetting("overlay_theme") ||
      "calm") as OverlayThemeValue;

    const options = [
      {
        value: "calm",
        label: t("settings.advanced.overlayTheme.options.calm"),
      },
      {
        value: "classic",
        label: t("settings.advanced.overlayTheme.options.classic"),
      },
    ];

    return (
      <SettingContainer
        title={t("settings.advanced.overlayTheme.title")}
        description={t("settings.advanced.overlayTheme.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <Dropdown
          options={options}
          selectedValue={selectedTheme}
          onSelect={(value) =>
            updateSetting("overlay_theme", value as OverlayThemeValue)
          }
          disabled={isUpdating("overlay_theme")}
        />
      </SettingContainer>
    );
  },
);
