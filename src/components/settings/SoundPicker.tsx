import React from "react";
import { Button } from "../ui/Button";
import { Dropdown, DropdownOption } from "../ui/Dropdown";
import { PlayIcon, UploadIcon } from "lucide-react";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettingsStore } from "../../stores/settingsStore";
import { useSettings } from "../../hooks/useSettings";

interface SoundPickerProps {
  soundType: "start" | "stop";
  label: string;
  description: string;
}

export const SoundPicker: React.FC<SoundPickerProps> = ({
  soundType,
  label,
  description,
}) => {
  const { getSetting, updateSetting } = useSettings();
  const playTestSound = useSettingsStore((state) => state.playTestSound);
  const uploadCustomSound = useSettingsStore(
    (state) => state.uploadCustomSound,
  );
  const customSounds = useSettingsStore((state) => state.customSounds);

  const selectedSound = getSetting(`${soundType}_sound`) ?? "default";
  const options: DropdownOption[] = [
    { value: "default", label: "Default" },
    { value: "pop", label: "Pop" },
    {
      value: "custom",
      label: "Custom",
      disabled: !customSounds[soundType],
    },
  ];

  return (
    <SettingContainer
      title={label}
      description={description}
      grouped
      layout="horizontal"
    >
      <div className="flex items-center">
        <Button
          variant="ghost"
          size="sm"
          onClick={() => uploadCustomSound(soundType)}
        >
          <UploadIcon className="h-4 w-4" />
        </Button>
        <Dropdown
          selectedValue={selectedSound}
          onSelect={(value) =>
            updateSetting(
              `${soundType}_sound`,
              value as "default" | "pop" | "custom",
            )
          }
          options={options}
        />
        <Button
          variant="ghost"
          size="sm"
          onClick={() => playTestSound(soundType)}
        >
          <PlayIcon className="h-4 w-4" />
        </Button>
      </div>
    </SettingContainer>
  );
};
