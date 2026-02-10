import React from "react";
import { useTranslation } from "react-i18next";
import { SettingContainer } from "../../ui/SettingContainer";
import { Dropdown, type DropdownOption } from "../../ui/Dropdown";
import { useSettings } from "../../../hooks/useSettings";
import type { LogLevel } from "../../../bindings";

const LOG_LEVEL_OPTIONS: DropdownOption[] = [
  { value: "error", label: "Error" },
  { value: "warn", label: "Warn" },
  { value: "info", label: "Info" },
  { value: "debug", label: "Debug" },
  { value: "trace", label: "Trace" },
];

interface LogLevelSelectorProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const LogLevelSelector: React.FC<LogLevelSelectorProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { settings, updateSetting, isUpdating } = useSettings();
  const currentLevel = settings?.log_level ?? "debug";

  const handleSelect = async (value: string) => {
    if (value === currentLevel) return;

    try {
      await updateSetting("log_level", value as LogLevel);
    } catch (error) {
      console.error("Failed to update log level:", error);
    }
  };

  return (
    <SettingContainer
      title={t("settings.debug.logLevel.title")}
      description={t("settings.debug.logLevel.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
      layout="horizontal"
    >
      <Dropdown
        options={LOG_LEVEL_OPTIONS}
        selectedValue={currentLevel}
        onSelect={handleSelect}
        disabled={!settings || isUpdating("log_level")}
      />
    </SettingContainer>
  );
};
