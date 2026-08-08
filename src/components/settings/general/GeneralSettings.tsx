import React from "react";
import { useTranslation } from "react-i18next";
import { type } from "@tauri-apps/plugin-os";
import { MicrophoneSelector } from "../MicrophoneSelector";
import { ChannelSelector } from "../ChannelSelector";
import { ShortcutInput } from "../ShortcutInput";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { OutputDeviceSelector } from "../OutputDeviceSelector";
import { PushToTalk } from "../PushToTalk";
import { AudioFeedback } from "../AudioFeedback";
import { ToggleSwitch } from "../../ui/ToggleSwitch";
import { useSettings } from "../../../hooks/useSettings";
import { VolumeSlider } from "../VolumeSlider";
import { MuteWhileRecording } from "../MuteWhileRecording";
import { ModelSettingsCard } from "./ModelSettingsCard";

export const GeneralSettings: React.FC = () => {
  const { t } = useTranslation();
  const { audioFeedbackEnabled, getSetting, updateSetting, isUpdating } =
    useSettings();
  const pushToTalk = getSetting("push_to_talk");
  const toggleShortcutEnabled =
    getSetting("transcribe_toggle_enabled") || false;
  const isLinux = type() === "linux";
  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("settings.general.title")}>
        <ShortcutInput shortcutId="transcribe" grouped={true} />
        <PushToTalk descriptionMode="tooltip" grouped={true} />
        {/* The toggle shortcut only complements push-to-talk (without it, the
            transcribe shortcut already toggles); disabling push-to-talk also
            disables it (see change_ptt_setting) */}
        {pushToTalk && (
          <ToggleSwitch
            checked={toggleShortcutEnabled}
            onChange={(enabled) =>
              updateSetting("transcribe_toggle_enabled", enabled)
            }
            isUpdating={isUpdating("transcribe_toggle_enabled")}
            label={t("settings.general.toggleShortcut.label")}
            description={t("settings.general.toggleShortcut.description")}
            descriptionMode="tooltip"
            grouped={true}
          />
        )}
        {pushToTalk && toggleShortcutEnabled && (
          <ShortcutInput shortcutId="transcribe_toggle" grouped={true} />
        )}
        {/* Cancel shortcut is hidden on Linux (dynamic shortcut instability) and when no shortcut can start a toggle-mode recording */}
        {!isLinux && (!pushToTalk || toggleShortcutEnabled) && (
          <ShortcutInput shortcutId="cancel" grouped={true} />
        )}
      </SettingsGroup>
      <ModelSettingsCard />
      <SettingsGroup title={t("settings.sound.title")}>
        <MicrophoneSelector descriptionMode="tooltip" grouped={true} />
        <ChannelSelector descriptionMode="tooltip" grouped={true} />
        <MuteWhileRecording descriptionMode="tooltip" grouped={true} />
        <AudioFeedback descriptionMode="tooltip" grouped={true} />
        <OutputDeviceSelector
          descriptionMode="tooltip"
          grouped={true}
          disabled={!audioFeedbackEnabled}
        />
        <VolumeSlider disabled={!audioFeedbackEnabled} />
      </SettingsGroup>
    </div>
  );
};
