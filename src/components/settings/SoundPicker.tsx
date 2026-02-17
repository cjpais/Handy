import React from "react";
import { Button } from "../ui/Button";
import { Dropdown, DropdownOption } from "../ui/Dropdown";
import { FolderOpenIcon, PlayIcon, Trash2Icon } from "lucide-react";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";

interface SoundPickerProps {
  label: string;
  description: string;
  disabled?: boolean;
}

interface CustomSoundPaths {
  start: string | null;
  stop: string | null;
}

export const SoundPicker: React.FC<SoundPickerProps> = ({
  label,
  description,
  disabled = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting } = useSettings();
  const [customPaths, setCustomPaths] = React.useState<CustomSoundPaths>({
    start: null,
    stop: null,
  });
  const [isChoosing, setIsChoosing] = React.useState<"start" | "stop" | null>(
    null,
  );
  const [isUpdating, setIsUpdating] = React.useState<"start" | "stop" | null>(
    null,
  );

  const selectedTheme = getSetting("sound_theme") ?? "marimba";

  const options: DropdownOption[] = React.useMemo(
    () => [
      { value: "marimba", label: "Marimba" },
      { value: "pop", label: "Pop" },
      { value: "custom", label: t("settings.sound.customSounds.themeOption") },
    ],
    [t],
  );

  const loadCustomSoundPaths = React.useCallback(async () => {
    try {
      const paths = await invoke<CustomSoundPaths>("get_custom_sound_paths");
      setCustomPaths({
        start: paths.start ?? null,
        stop: paths.stop ?? null,
      });
    } catch (error) {
      console.error("Failed to load custom sound paths:", error);
    }
  }, []);

  React.useEffect(() => {
    void loadCustomSoundPaths();
  }, [loadCustomSoundPaths]);

  const playTestSound = async (soundType: "start" | "stop") => {
    try {
      await invoke("play_test_sound", { soundType });
    } catch (error) {
      console.error(`Failed to play test sound (${soundType}):`, error);
    }
  };

  const setCustomSoundPath = async (
    soundType: "start" | "stop",
    path: string | null,
  ) => {
    setIsUpdating(soundType);
    try {
      await invoke("set_custom_sound_path", { soundType, path });
      await loadCustomSoundPaths();
    } catch (error) {
      console.error(`Failed to set custom ${soundType} sound path:`, error);
    } finally {
      setIsUpdating(null);
    }
  };

  const selectCustomSound = async (soundType: "start" | "stop") => {
    setIsChoosing(soundType);
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [
          {
            name: "Audio",
            extensions: ["wav", "mp3", "ogg", "flac", "m4a", "aac"],
          },
        ],
      });

      if (!selected || Array.isArray(selected)) {
        return;
      }

      await setCustomSoundPath(soundType, selected);
      if (selectedTheme !== "custom") {
        await updateSetting("sound_theme", "custom");
      }
    } finally {
      setIsChoosing(null);
    }
  };

  const handlePlayBothSounds = async () => {
    await playTestSound("start");
    await playTestSound("stop");
  };

  const renderCustomSoundRow = (soundType: "start" | "stop") => {
    const path = customPaths[soundType];
    const isBusy = isChoosing === soundType || isUpdating === soundType;
    const fileName = path ? path.split(/[\\/]/).pop() || path : null;
    const canPreview = Boolean(path);

    return (
      <div
        className="grid grid-cols-[56px_minmax(0,1fr)] items-center gap-2"
        key={soundType}
      >
        <div className="text-xs font-medium text-text/80" title={soundType}>
          {soundType === "start"
            ? t("settings.sound.customSounds.startLabel")
            : t("settings.sound.customSounds.stopLabel")}
        </div>
        <div className="flex items-center gap-2 min-w-0">
          <div
            className="flex-1 min-w-0 px-2 py-[6px] bg-mid-gray/10 border border-mid-gray/50 rounded-md text-xs truncate"
            title={path ?? ""}
          >
            {fileName ?? t("settings.sound.customSounds.notSelected")}
          </div>
          <div className="flex items-center rounded-md border border-mid-gray/30 bg-mid-gray/5 p-1">
            <Button
              variant="ghost"
              size="sm"
              className="h-7 w-7 p-0 flex items-center justify-center"
              onClick={() => void selectCustomSound(soundType)}
              disabled={disabled || isBusy}
              title={t("settings.sound.customSounds.choose")}
              aria-label={t("settings.sound.customSounds.choose")}
            >
              <FolderOpenIcon className="h-4 w-4" />
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className="h-7 w-7 p-0 flex items-center justify-center"
              onClick={() => void setCustomSoundPath(soundType, null)}
              disabled={disabled || isBusy || !path}
              title={t("common.remove")}
              aria-label={t("common.remove")}
            >
              <Trash2Icon className="h-4 w-4" />
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className="h-7 w-7 p-0 flex items-center justify-center"
              onClick={() => void playTestSound(soundType)}
              disabled={disabled || !canPreview}
              title={t("settings.sound.customSounds.preview")}
            >
              <PlayIcon className="h-4 w-4" />
            </Button>
          </div>
        </div>
      </div>
    );
  };

  return (
    <SettingContainer
      title={label}
      description={description}
      grouped
      layout="stacked"
      disabled={disabled}
    >
      <div className="space-y-2">
        <div className="flex items-center gap-2">
          <Dropdown
            selectedValue={selectedTheme}
            onSelect={(value) =>
              updateSetting(
                "sound_theme",
                value as "marimba" | "pop" | "custom",
              )
            }
            options={options}
            disabled={disabled}
          />
          <Button
            variant="ghost"
            size="sm"
            onClick={handlePlayBothSounds}
            title={t("settings.sound.customSounds.previewTheme")}
            disabled={disabled}
          >
            <PlayIcon className="h-4 w-4" />
          </Button>
        </div>

        {selectedTheme === "custom" && (
          <div className="space-y-2 rounded-lg border border-mid-gray/20 bg-mid-gray/5 p-3">
            {["start", "stop"].map((s) =>
              renderCustomSoundRow(s as "start" | "stop"),
            )}
          </div>
        )}
      </div>
    </SettingContainer>
  );
};
