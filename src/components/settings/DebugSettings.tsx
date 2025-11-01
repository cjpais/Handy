import React from "react";
import { WordCorrectionThreshold } from "./debug/WordCorrectionThreshold";
import { SettingsGroup } from "../ui/SettingsGroup";
import { HistoryLimit } from "./HistoryLimit";
import { PasteMethodSetting } from "./PasteMethod";
import { ClipboardHandlingSetting } from "./ClipboardHandling";
import { AlwaysOnMicrophone } from "./AlwaysOnMicrophone";
import { SoundPicker } from "./SoundPicker";
import { MuteWhileRecording } from "./MuteWhileRecording";
import { TranscriptionSourceSetting } from "./TranscriptionSource";
import { ApiKeySetting } from "./ApiKey";
import { ApiEndpointSetting } from "./ApiEndpoint";
import { ApiModelSetting } from "./ApiModel";
import { ApiPromptSetting } from "./ApiPrompt";

export const DebugSettings: React.FC = () => {
  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title="API Configuration">
        <TranscriptionSourceSetting descriptionMode="tooltip" grouped={true} />
        <ApiKeySetting descriptionMode="tooltip" grouped={true} />
        <ApiModelSetting descriptionMode="tooltip" grouped={true} />
        <ApiEndpointSetting descriptionMode="tooltip" grouped={true} />
        <ApiPromptSetting descriptionMode="tooltip" grouped={true} />
      </SettingsGroup>

      <SettingsGroup title="Debug">
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
