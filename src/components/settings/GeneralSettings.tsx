import React from "react";
import { MicrophoneSelector } from "./MicrophoneSelector";
import { HandyShortcut } from "./HandyShortcut";
import { SettingsGroup } from "../ui/SettingsGroup";
import { OutputDeviceSelector } from "./OutputDeviceSelector";
import { PushToTalk } from "./PushToTalk";
import { AudioFeedback } from "./AudioFeedback";
import { useSettings } from "../../hooks/useSettings";
import { useModels } from "../../hooks/useModels";

export const GeneralSettings: React.FC = () => {
  const { audioFeedbackEnabled } = useSettings();
  const { isParakeetModel } = useModels();

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title="General">
        <HandyShortcut
          descriptionMode="tooltip"
          grouped={true}
          disableLanguageSelection={isParakeetModel}
        />
        <PushToTalk descriptionMode="tooltip" grouped={true} />
      </SettingsGroup>
      <SettingsGroup title="Sound">
        <MicrophoneSelector descriptionMode="tooltip" grouped={true} />
        <AudioFeedback descriptionMode="tooltip" grouped={true} />
        <OutputDeviceSelector
          descriptionMode="tooltip"
          grouped={true}
          disabled={!audioFeedbackEnabled}
        />
      </SettingsGroup>
    </div>
  );
};
