import React from "react";
import { Button } from "../ui/Button";
import { Dropdown, DropdownOption } from "../ui/Dropdown";
import { PlayIcon } from "lucide-react";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettingsStore } from "../../stores/settingsStore";
import { useSettings } from "../../hooks/useSettings";
import { useTranslation } from "react-i18next";
import type { SoundTheme } from "@/bindings";

interface SoundPickerProps {
  label: string;
  description: string;
  disabled?: boolean;
}

export const SoundPicker: React.FC<SoundPickerProps> = ({
  label,
  description,
  disabled = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting } = useSettings();
  const playTestSound = useSettingsStore((state) => state.playTestSound);
  const customSounds = useSettingsStore((state) => state.customSounds);

  const selectedTheme = getSetting("sound_theme") ?? "marimba";

  const themeList: SoundTheme[] = ["marimba", "pop", "bell", "chime", "pluck"];
  const themeLabels: Record<SoundTheme, string> = {
    marimba: t("settings.sound.soundTheme.options.marimba"),
    pop: t("settings.sound.soundTheme.options.pop"),
    bell: t("settings.sound.soundTheme.options.bell"),
    chime: t("settings.sound.soundTheme.options.chime"),
    pluck: t("settings.sound.soundTheme.options.pluck"),
    custom: t("settings.sound.soundTheme.options.custom"),
  };

  const options: DropdownOption[] = [
    ...themeList.map((theme) => ({
      value: theme,
      label: themeLabels[theme],
    })),
  ];

  // Only add Custom option if both custom sound files exist
  if (customSounds.start && customSounds.stop) {
    options.push({
      value: "custom",
      label: themeLabels.custom,
    });
  }

  const handlePlayBothSounds = async () => {
    await playTestSound("start");
    await playTestSound("stop");
  };

  return (
    <SettingContainer
      title={label}
      description={description}
      disabled={disabled}
      grouped
      layout="horizontal"
    >
      <div className="flex items-center gap-2">
        <Dropdown
          selectedValue={selectedTheme}
          onSelect={(value) =>
            updateSetting("sound_theme", value as SoundTheme)
          }
          options={options}
          disabled={disabled}
        />
        <Button
          variant="ghost"
          size="sm"
          onClick={handlePlayBothSounds}
          title={t("settings.sound.soundTheme.preview")}
          disabled={disabled}
        >
          <PlayIcon className="h-4 w-4" />
        </Button>
      </div>
    </SettingContainer>
  );
};
