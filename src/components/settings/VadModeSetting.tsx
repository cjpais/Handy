import React from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import type { VadMode } from "@/bindings";

interface VadModeSettingProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const VadModeSetting: React.FC<VadModeSettingProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const selectedMode = (getSetting("vad_mode") || "filter") as VadMode;

    const options = [
      {
        value: "off",
        label: t("settings.debug.vadMode.options.off"),
      },
      {
        value: "filter",
        label: t("settings.debug.vadMode.options.filter"),
      },
      {
        value: "stream",
        label: t("settings.debug.vadMode.options.stream"),
      },
      {
        value: "batch_stream",
        label: t("settings.debug.vadMode.options.batchStream"),
      },
    ];

    return (
      <SettingContainer
        title={t("settings.debug.vadMode.title")}
        description={t("settings.debug.vadMode.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
        tooltipPosition="bottom"
      >
        <Dropdown
          options={options}
          selectedValue={selectedMode}
          onSelect={(value) => updateSetting("vad_mode", value as VadMode)}
          disabled={isUpdating("vad_mode")}
        />
      </SettingContainer>
    );
  },
);
