import React from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "../../hooks/useSettings";
import type { AutoStopSilenceTimeout } from "@/bindings";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";

interface AutoStopSilenceTimeoutProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const AutoStopSilenceTimeoutSetting: React.FC<
  AutoStopSilenceTimeoutProps
> = ({ descriptionMode = "inline", grouped = false }) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();

  const timeoutOptions = [
    {
      value: "disabled" as AutoStopSilenceTimeout,
      label: t("settings.advanced.autoStopSilence.options.disabled"),
    },
    {
      value: "sec2" as AutoStopSilenceTimeout,
      label: t("settings.advanced.autoStopSilence.options.sec2"),
    },
    {
      value: "sec3" as AutoStopSilenceTimeout,
      label: t("settings.advanced.autoStopSilence.options.sec3"),
    },
    {
      value: "sec5" as AutoStopSilenceTimeout,
      label: t("settings.advanced.autoStopSilence.options.sec5"),
    },
    {
      value: "sec10" as AutoStopSilenceTimeout,
      label: t("settings.advanced.autoStopSilence.options.sec10"),
    },
  ];

  const currentValue = getSetting("auto_stop_silence_timeout") ?? "disabled";

  return (
    <SettingContainer
      title={t("settings.advanced.autoStopSilence.title")}
      description={t("settings.advanced.autoStopSilence.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <Dropdown
        options={timeoutOptions}
        selectedValue={currentValue}
        onSelect={(value) =>
          updateSetting("auto_stop_silence_timeout", value as AutoStopSilenceTimeout)
        }
        disabled={isUpdating("auto_stop_silence_timeout")}
      />
    </SettingContainer>
  );
};
