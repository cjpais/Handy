import React, { useState } from "react";
import { MicrophoneSelector } from "./MicrophoneSelector";
import { AlwaysOnMicrophone } from "./AlwaysOnMicrophone";
import { PushToTalk } from "./PushToTalk";
import { AudioFeedback } from "./AudioFeedback";
import { OutputDeviceSelector } from "./OutputDeviceSelector";
import { HandyShortcut } from "./HandyShortcut";
import { TranslateToEnglish } from "./TranslateToEnglish";
import { SettingsGroup } from "../ui/SettingsGroup";
import { invoke } from "@tauri-apps/api/core";

export const Settings: React.FC = () => {
  const [isTestRecording, setIsTestRecording] = useState(false);

  const handleTestRecording = async () => {
    if (isTestRecording) {
      try {
        await invoke("test_stop_recording");
        setIsTestRecording(false);
      } catch (error) {
        console.error("Failed to stop test recording:", error);
      }
    } else {
      try {
        const success = await invoke("test_start_recording");
        if (success) {
          setIsTestRecording(true);
        }
      } catch (error) {
        console.error("Failed to start test recording:", error);
      }
    }
  };

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup>
        <HandyShortcut descriptionMode="tooltip" grouped={true} />
        <MicrophoneSelector descriptionMode="tooltip" grouped={true} />
      </SettingsGroup>

      <SettingsGroup title="Advanced">
        <PushToTalk descriptionMode="tooltip" grouped={true} />
        <AudioFeedback descriptionMode="tooltip" grouped={true} />
        <OutputDeviceSelector descriptionMode="tooltip" grouped={true} />
        <TranslateToEnglish descriptionMode="tooltip" grouped={true} />
        <AlwaysOnMicrophone descriptionMode="tooltip" grouped={true} />
      </SettingsGroup>

      <SettingsGroup title="Testing">
        <div className="flex items-center justify-between py-2">
          <div>
            <div className="text-sm font-medium">
              Test Voice Activity Indicator
            </div>
            <div className="text-xs text-mid-gray">
              Test the voice activity bubble display
            </div>
          </div>
          <button
            onClick={handleTestRecording}
            className={`px-4 py-2 rounded-lg text-sm font-medium transition-colors ${
              isTestRecording
                ? "bg-red-500 hover:bg-red-600 text-white"
                : "bg-logo-primary hover:bg-logo-primary/80 text-white"
            }`}
          >
            {isTestRecording ? "Stop Test" : "Start Test"}
          </button>
        </div>
      </SettingsGroup>
    </div>
  );
};
