import React from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "../../../hooks/useSettings";
import { SettingContainer } from "../../ui/SettingContainer";
import type { FillerOutputMode } from "@/bindings";

const OUTPUT_MODES: { value: FillerOutputMode; labelKey: string }[] = [
  { value: "coaching_only", labelKey: "coaching.outputMode.coachingOnly" },
  { value: "paste_cleaned", labelKey: "coaching.outputMode.pasteCleaned" },
  { value: "paste_original", labelKey: "coaching.outputMode.pasteOriginal" },
  { value: "both", labelKey: "coaching.outputMode.both" },
];

interface FillerOutputModeSelectorProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const FillerOutputModeSelector: React.FC<FillerOutputModeSelectorProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const currentMode = getSetting("filler_output_mode") ?? "coaching_only";
    const updating = isUpdating("filler_output_mode");

    const handleChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
      updateSetting("filler_output_mode", e.target.value as FillerOutputMode);
    };

    return (
      <SettingContainer
        title={t("coaching.outputMode.label")}
        description={t("coaching.outputMode.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <select
          value={currentMode}
          onChange={handleChange}
          disabled={updating}
          className="px-2 py-1 text-sm font-semibold bg-mid-gray/10 border border-mid-gray/80 rounded min-w-[200px] text-left transition-all duration-150 hover:bg-logo-primary/10 cursor-pointer hover:border-logo-primary disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {OUTPUT_MODES.map((mode) => (
            <option key={mode.value} value={mode.value}>
              {t(mode.labelKey)}
            </option>
          ))}
        </select>
      </SettingContainer>
    );
  },
);
