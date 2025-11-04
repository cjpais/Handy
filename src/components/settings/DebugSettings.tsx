import React from "react";
import { useTranslation } from "react-i18next";
import { WordCorrectionThreshold } from "./debug/WordCorrectionThreshold";
import { SettingsGroup } from "../ui/SettingsGroup";
import { HistoryLimit } from "./HistoryLimit";
import { AlwaysOnMicrophone } from "./AlwaysOnMicrophone";
import { SoundPicker } from "./SoundPicker";
import { MuteWhileRecording } from "./MuteWhileRecording";

export const DebugSettings: React.FC = () => {
  const { t } = useTranslation();

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("settings.groups.debug")}>
        <PasteMethodSetting descriptionMode="tooltip" grouped={true} />
        <ClipboardHandlingSetting descriptionMode="tooltip" grouped={true} />
        <SoundPicker />
        <WordCorrectionThreshold descriptionMode="tooltip" grouped={true} />
        <HistoryLimit descriptionMode="tooltip" grouped={true} />
        <AlwaysOnMicrophone descriptionMode="tooltip" grouped={true} />
        <MuteWhileRecording descriptionMode="tooltip" grouped={true} />
      </SettingsGroup>
    </div>
  );
};
