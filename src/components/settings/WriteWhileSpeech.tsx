import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { SettingContainer } from "../ui/SettingContainer";
import { Input } from "../ui/Input";
import { useSettings } from "../../hooks/useSettings";

interface WriteWhileSpeechProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const WriteWhileSpeech: React.FC<WriteWhileSpeechProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("write_while_speech") ?? false;
    const writeDelayMs = getSetting("write_delay_ms") ?? 200;

    const handleDelayChange = (event: React.ChangeEvent<HTMLInputElement>) => {
      const value = Number.parseInt(event.target.value, 10);
      if (!Number.isNaN(value) && value >= 50 && value <= 5000) {
        updateSetting("write_delay_ms", value);
      }
    };

    return (
      <>
        <ToggleSwitch
          checked={enabled}
          onChange={(value) => updateSetting("write_while_speech", value)}
          isUpdating={isUpdating("write_while_speech")}
          label={t("settings.advanced.writeAfterSilence.label")}
          description={t("settings.advanced.writeAfterSilence.description")}
          descriptionMode={descriptionMode}
          grouped={grouped}
        />
        <SettingContainer
          title={t("settings.advanced.writeAfterSilenceDelay.title")}
          description={t("settings.advanced.writeAfterSilenceDelay.description")}
          descriptionMode={descriptionMode}
          grouped={grouped}
          layout="horizontal"
          disabled={!enabled}
        >
          <div className="flex items-center space-x-2">
            <Input
              type="number"
              min="50"
              max="5000"
              step="10"
              value={writeDelayMs}
              onChange={handleDelayChange}
              disabled={!enabled || isUpdating("write_delay_ms")}
              className="w-24"
            />
            <span className="text-sm text-text">
              {t("settings.advanced.writeAfterSilenceDelay.unit")}
            </span>
          </div>
        </SettingContainer>
      </>
    );
  },
);
