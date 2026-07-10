import React from "react";
import { useTranslation } from "react-i18next";
import { SettingsGroup } from "../ui/SettingsGroup";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { Slider } from "../ui/Slider";
import { useSettings } from "../../hooks/useSettings";

export const VoiceEdit: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();

  const enabled = getSetting("voice_edit_enabled") || false;
  const windowSecs = getSetting("voice_edit_window_secs") ?? 30;
  const autoSubmit = getSetting("auto_submit") || false;

  return (
    <SettingsGroup title={t("settings.voiceEdit.title")}>
      <ToggleSwitch
        checked={enabled}
        onChange={(value) => updateSetting("voice_edit_enabled", value)}
        isUpdating={isUpdating("voice_edit_enabled")}
        label={t("settings.voiceEdit.enabled.label")}
        description={t("settings.voiceEdit.enabled.description")}
        descriptionMode="tooltip"
        grouped
      />
      <Slider
        value={windowSecs}
        onChange={(value) => updateSetting("voice_edit_window_secs", value)}
        min={10}
        max={120}
        step={5}
        label={t("settings.voiceEdit.window.title")}
        description={t("settings.voiceEdit.window.description")}
        descriptionMode="tooltip"
        grouped
        formatValue={(value) => `${value} s`}
        disabled={!enabled}
      />
      {enabled && autoSubmit && (
        <p className="text-xs text-yellow-400 px-4 pb-3">
          {t("settings.voiceEdit.autoSubmitWarning")}
        </p>
      )}
    </SettingsGroup>
  );
};
