import { Button } from "../ui/Button";
import { Dropdown } from "../ui/Dropdown";
import { PlayIcon, UploadIcon } from "lucide-react";
import React from "react";
import { SettingContainer } from "../ui/SettingContainer";
import { Slider } from "../ui/Slider";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";
import { useSettingsStore } from "../../stores/settingsStore";

interface AudioFeedbackProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const AudioFeedback: React.FC<AudioFeedbackProps> = React.memo(({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const playTestSound = useSettingsStore((state) => state.playTestSound);
  const uploadCustomSound = useSettingsStore(
    (state) => state.uploadCustomSound,
  );
  const customSounds = useSettingsStore((state) => state.customSounds);

  const audioFeedbackEnabled = getSetting("audio_feedback") || false;
  const audioFeedbackVolume = getSetting("audio_feedback_volume") ?? 0.5;
  const startSound = getSetting("start_sound") ?? "default";
  const stopSound = getSetting("stop_sound") ?? "default";

  return (
    <div className="flex flex-col">
      <ToggleSwitch
        checked={audioFeedbackEnabled}
        onChange={(enabled) => updateSetting("audio_feedback", enabled)}
        isUpdating={isUpdating("audio_feedback")}
        label="Audio Feedback"
        description="Play sound when recording starts and stops"
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
      {audioFeedbackEnabled && (
        <div className="pl-6 pt-1 flex flex-col space-y-1">
          <SettingContainer
            title="Start Sound"
            description="Sound played when the transcription starts"
            grouped
            layout="horizontal"
          >
            <div className="flex items-center">
              <Button
                variant="ghost"
                size="sm"
                onClick={() => uploadCustomSound("start")}
              >
                <UploadIcon className="h-4 w-4" />
              </Button>
              <Dropdown
                selectedValue={startSound}
                onSelect={(value) => updateSetting("start_sound", value as "default" | "v2" | "v3")}
                options={[
                  { value: "default", label: "Default" },
                  { value: "pop", label: "Pop" },
                  {
                    value: "custom",
                    label: "Custom",
                    disabled: !customSounds.start,
                  },
                ]}
              />
              <Button
                variant="ghost"
                size="sm"
                onClick={() => playTestSound("start")}
              >
                <PlayIcon className="h-4 w-4" />
              </Button>
            </div>
          </SettingContainer>
          <SettingContainer
            title="Stop Sound"
            description="Sound played when the transcription ends"
            grouped
            layout="horizontal"
          >
            <div className="flex items-center">
              <Button
                variant="ghost"
                size="sm"
                onClick={() => uploadCustomSound("stop")}
              >
                <UploadIcon className="h-4 w-4" />
              </Button>
              <Dropdown
                selectedValue={stopSound}
                onSelect={(value) => updateSetting("stop_sound", value as "default" | "v2" | "v3")}
                options={[
                  { value: "default", label: "Default" },
                  { value: "pop", label: "Pop" },
                  {
                    value: "custom",
                    label: "Custom",
                    disabled: !customSounds.stop,
                  },
                ]}
              />
              <Button
                variant="ghost"
                size="sm"
                onClick={() => playTestSound("stop")}
              >
                <PlayIcon className="h-4 w-4" />
              </Button>
            </div>
          </SettingContainer>
          <Slider
            value={audioFeedbackVolume}
            onChange={(value: number) =>
              updateSetting("audio_feedback_volume", value)
            }
            min={0}
            max={1}
            step={0.1}
            label="Volume"
            descriptionMode="inline"
            grouped
            formatValue={(value) => `${Math.round(value * 100)}%`}
          />
        </div>
      )}
    </div>
  );
});
