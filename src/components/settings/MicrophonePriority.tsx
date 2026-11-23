import React from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, ChevronUp, X } from "lucide-react";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";

interface MicrophonePriorityProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

const ICON_BUTTON =
  "grid size-[20px] place-items-center rounded-[6px] text-ink-3 focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary enabled:cursor-pointer enabled:hover:bg-line enabled:hover:text-text disabled:opacity-40";

/** The microphones in order of preference: Handy records from the first one plugged in. */
export const MicrophonePriority: React.FC<MicrophonePriorityProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const {
    getSetting,
    updateSetting,
    isUpdating,
    isLoading,
    audioDevices,
    refreshAudioDevices,
  } = useSettings();

  const priority = getSetting("microphone_priority") ?? [];
  const busy = isUpdating("microphone_priority") || isLoading;
  const connected = audioDevices.map((device) => device.name);
  const save = (next: string[]) => updateSetting("microphone_priority", next);

  const move = (index: number, by: -1 | 1) => {
    const next = [...priority];
    [next[index], next[index + by]] = [next[index + by], next[index]];
    save(next);
  };

  const addable = audioDevices
    .filter((device) => !device.is_default && !priority.includes(device.name))
    .map((device) => ({ value: device.name, label: device.name }));

  return (
    <SettingContainer
      title={t("settings.sound.microphonePriority.title")}
      description={t("settings.sound.microphonePriority.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
      layout="stacked"
    >
      <ol className="mb-2 space-y-1">
        {priority.map((name, index) => {
          // Before the device list loads, nothing is known to be missing.
          const missing = audioDevices.length > 0 && !connected.includes(name);
          return (
            <li
              key={name}
              className="flex items-center gap-2 rounded-md border border-line px-2 py-1 text-sm"
            >
              <span className="w-4 text-end tabular-nums text-ink-3">
                {index + 1}
              </span>
              <span className={`truncate ${missing ? "text-ink-3" : ""}`}>
                {name}
              </span>
              {missing && (
                <span className="text-xs whitespace-nowrap text-ink-3">
                  {t("settings.sound.microphonePriority.notConnected")}
                </span>
              )}
              <span className="ms-auto flex items-center gap-1">
                <button
                  type="button"
                  className={ICON_BUTTON}
                  aria-label={t("settings.sound.microphonePriority.moveUp", {
                    name,
                  })}
                  disabled={busy || index === 0}
                  onClick={() => move(index, -1)}
                >
                  <ChevronUp size={14} aria-hidden="true" />
                </button>
                <button
                  type="button"
                  className={ICON_BUTTON}
                  aria-label={t("settings.sound.microphonePriority.moveDown", {
                    name,
                  })}
                  disabled={busy || index === priority.length - 1}
                  onClick={() => move(index, 1)}
                >
                  <ChevronDown size={14} aria-hidden="true" />
                </button>
                <button
                  type="button"
                  className={ICON_BUTTON}
                  aria-label={t("settings.sound.microphonePriority.remove", {
                    name,
                  })}
                  disabled={busy}
                  onClick={() => save(priority.filter((n) => n !== name))}
                >
                  <X size={14} aria-hidden="true" />
                </button>
              </span>
            </li>
          );
        })}
      </ol>
      <Dropdown
        options={addable}
        selectedValue={null}
        onSelect={(name) => save([...priority, name])}
        placeholder={t("settings.sound.microphonePriority.add")}
        disabled={busy}
        onOpen={refreshAudioDevices}
      />
    </SettingContainer>
  );
};
