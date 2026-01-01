import React from "react";
import { useTranslation } from "react-i18next";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { FillerDetectionToggle } from "./FillerDetectionToggle";
import { FillerOutputModeSelector } from "./FillerOutputModeSelector";
import { CustomFillerWords } from "./CustomFillerWords";
import { ShowFillerOverlay } from "./ShowFillerOverlay";
import { useSettings } from "../../../hooks/useSettings";

export const CoachingSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting } = useSettings();

  const fillerDetectionEnabled = getSetting("filler_detection_enabled") ?? false;

  return (
    <div className="flex flex-col gap-4">
      <SettingsGroup title={t("coaching.title")}>
        <FillerDetectionToggle descriptionMode="tooltip" grouped />

        {fillerDetectionEnabled && (
          <>
            <FillerOutputModeSelector descriptionMode="tooltip" grouped />
            <ShowFillerOverlay descriptionMode="tooltip" grouped />
            <CustomFillerWords grouped />
          </>
        )}
      </SettingsGroup>
    </div>
  );
};
