import React, { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { AudioBackend } from "@/bindings";
import { commands } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { useOsType } from "@/hooks/useOsType";
import { Dropdown, type DropdownOption } from "@/components/ui/Dropdown";
import { SettingContainer } from "@/components/ui/SettingContainer";

interface AudioBackendSelectorProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const AudioBackendSelector: React.FC<AudioBackendSelectorProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const osType = useOsType();
  const [resolvedBackend, setResolvedBackend] = useState<string | null>(null);

  const selectedBackend = getSetting("audio_backend") ?? "auto";

  // "Auto" hides which backend actually won resolution, so show it. The backend
  // is fixed at startup, so this only needs fetching once per settings visit.
  useEffect(() => {
    if (osType !== "linux") return;
    commands
      .getResolvedAudioBackend()
      .then((result) => {
        setResolvedBackend(result.status === "ok" ? result.data : null);
      })
      .catch(() => setResolvedBackend(null));
  }, [osType, selectedBackend]);

  const options = useMemo<DropdownOption[]>(
    () => [
      {
        value: "auto",
        label: t("settings.advanced.audioBackend.options.auto"),
      },
      {
        value: "pipewire",
        label: t("settings.advanced.audioBackend.options.pipewire"),
      },
      {
        value: "alsa",
        label: t("settings.advanced.audioBackend.options.alsa"),
      },
    ],
    [t],
  );

  // The capture backend is only selectable on Linux; cpal is the only option
  // on Windows and macOS.
  if (osType !== "linux") {
    return null;
  }

  const activeLabel =
    resolvedBackend === "pipewire"
      ? t("settings.advanced.audioBackend.options.pipewire")
      : resolvedBackend === "alsa"
        ? t("settings.advanced.audioBackend.options.alsa")
        : null;

  const description = activeLabel
    ? `${t("settings.advanced.audioBackend.description")} ${t(
        "settings.advanced.audioBackend.active",
        { backend: activeLabel },
      )}`
    : t("settings.advanced.audioBackend.description");

  return (
    <SettingContainer
      title={t("settings.advanced.audioBackend.title")}
      description={description}
      descriptionMode={descriptionMode}
      grouped={grouped}
      layout="horizontal"
    >
      <Dropdown
        options={options}
        selectedValue={selectedBackend}
        onSelect={(value) =>
          updateSetting("audio_backend", value as AudioBackend)
        }
        disabled={isUpdating("audio_backend")}
      />
    </SettingContainer>
  );
};
