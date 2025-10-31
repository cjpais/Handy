import React from "react";
import { WordCorrectionThreshold } from "./debug/WordCorrectionThreshold";
import { SettingsGroup } from "../ui/SettingsGroup";
import { HistoryLimit } from "./HistoryLimit";
import { PasteMethodSetting } from "./PasteMethod";
import { ClipboardHandlingSetting } from "./ClipboardHandling";
import { AlwaysOnMicrophone } from "./AlwaysOnMicrophone";
import { SoundPicker } from "./SoundPicker";
import { MuteWhileRecording } from "./MuteWhileRecording";
import { GeminiApiKey } from "./GeminiApiKey";

export const DebugSettings: React.FC = () => {
  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title="Debug">
        <GeminiApiKey descriptionMode="tooltip" grouped />
        <PasteMethodSetting descriptionMode="tooltip" grouped={true} />
        <ClipboardHandlingSetting descriptionMode="tooltip" grouped={true} />
        <SoundPicker
          label="Sound Theme"
          description="Choose a sound theme for recording start and stop feedback"
        />
        <WordCorrectionThreshold descriptionMode="tooltip" grouped={true} />
        <HistoryLimit descriptionMode="tooltip" grouped={true} />
        <AlwaysOnMicrophone descriptionMode="tooltip" grouped={true} />
        <MuteWhileRecording descriptionMode="tooltip" grouped={true} />
      </SettingsGroup>
    </div>
  );
};
