import React from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import type { MediaWhileRecordingMode } from "@/bindings";

interface MediaWhileRecordingProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const MediaWhileRecording: React.FC<MediaWhileRecordingProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const mode =
      (getSetting("media_while_recording_mode") as MediaWhileRecordingMode) ??
      "none";

    const options = [
      {
        value: "none",
        label: t("settings.general.mediaWhileRecording.options.none"),
      },
      {
        value: "mute",
        label: t("settings.general.mediaWhileRecording.options.mute"),
      },
      {
        value: "pause",
        label: t("settings.general.mediaWhileRecording.options.pause"),
      },
      {
        value: "fade",
        label: t("settings.general.mediaWhileRecording.options.fade"),
      },
    ];

    return (
      <SettingContainer
        title={t("settings.general.mediaWhileRecording.label")}
        description={t("settings.general.mediaWhileRecording.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <Dropdown
          options={options}
          selectedValue={mode}
          onSelect={(value) =>
            updateSetting(
              "media_while_recording_mode",
              value as MediaWhileRecordingMode,
            )
          }
          disabled={isUpdating("media_while_recording_mode")}
        />
      </SettingContainer>
    );
  });
