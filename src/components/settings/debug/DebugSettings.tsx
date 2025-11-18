import React from "react";
import { WordCorrectionThreshold } from "./WordCorrectionThreshold";
import { LogDirectory } from "./LogDirectory";
import { LogLevelSelector } from "./LogLevelSelector";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { HistoryLimit } from "../HistoryLimit";
import { AlwaysOnMicrophone } from "../AlwaysOnMicrophone";
import { SoundPicker } from "../SoundPicker";
import { PostProcessingToggle } from "../PostProcessingToggle";
import { MuteWhileRecording } from "../MuteWhileRecording";
import { RecordingRetentionPeriodSelector } from "../RecordingRetentionPeriod";
import { ClamshellMicrophoneSelector } from "../ClamshellMicrophoneSelector";

export const DebugSettings: React.FC = () => {
  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title="Debug">
        <LogDirectory grouped={true} />
        <LogLevelSelector grouped={true} />
        <SoundPicker
          label="Sound Theme"
          description="Choose a sound theme for recording start and stop feedback"
        />
        <WordCorrectionThreshold descriptionMode="tooltip" grouped={true} />
        <HistoryLimit descriptionMode="tooltip" grouped={true} />
        <RecordingRetentionPeriodSelector
          descriptionMode="tooltip"
          grouped={true}
        />
        <AlwaysOnMicrophone descriptionMode="tooltip" grouped={true} />
        <ClamshellMicrophoneSelector descriptionMode="tooltip" grouped={true} />
        <PostProcessingToggle descriptionMode="tooltip" grouped={true} />
        <MuteWhileRecording descriptionMode="tooltip" grouped={true} />
      </SettingsGroup>
    </div>
  );
};
