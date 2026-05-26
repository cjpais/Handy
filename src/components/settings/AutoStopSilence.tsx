import React from "react";
import { useTranslation } from "react-i18next";
import { Input } from "../ui/Input";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";

interface AutoStopSilenceProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const AutoStopSilence: React.FC<AutoStopSilenceProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("auto_stop_silence_enabled") ?? false;
    const seconds = getSetting("auto_stop_silence_seconds") ?? 5;

    const setSeconds = (event: React.ChangeEvent<HTMLInputElement>) => {
      const value = Number.parseInt(event.target.value, 10);
      if (Number.isNaN(value)) {
        return;
      }
      updateSetting(
        "auto_stop_silence_seconds",
        Math.min(30, Math.max(1, value)),
      );
    };

    return (
      <SettingContainer
        title={t("settings.advanced.autoStopSilence.title")}
        description={t("settings.advanced.autoStopSilence.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="flex items-center gap-2">
          <input
            type="checkbox"
            className="h-4 w-4 accent-logo-primary"
            checked={enabled}
            disabled={isUpdating("auto_stop_silence_enabled")}
            aria-label={t("settings.advanced.autoStopSilence.title")}
            onChange={(event) =>
              updateSetting("auto_stop_silence_enabled", event.target.checked)
            }
          />
          <span className="text-sm whitespace-nowrap">
            {t("settings.advanced.autoStopSilence.after")}
          </span>
          <Input
            type="number"
            min={1}
            max={30}
            value={seconds}
            onChange={setSeconds}
            disabled={!enabled || isUpdating("auto_stop_silence_seconds")}
            variant="compact"
            className="w-16 text-center"
          />
          <span className="text-sm whitespace-nowrap">
            {t("settings.advanced.autoStopSilence.seconds")}
          </span>
        </div>
      </SettingContainer>
    );
  },
);
