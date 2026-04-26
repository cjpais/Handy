import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { ResetButton } from "../ui/ResetButton";
import { useSettings } from "../../hooks/useSettings";

interface MicrophoneSelectorProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const MicrophoneSelector: React.FC<MicrophoneSelectorProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const {
      getSetting,
      updateSetting,
      resetSetting,
      isUpdating,
      isLoading,
      audioDevices,
      refreshAudioDevices,
    } = useSettings();

    // The product is bundled with an AI mouse whose receiver carries the
    // microphone signal. When the receiver is plugged in, recording is
    // hard-locked to that endpoint (the backend forces it regardless of
    // settings) — so the picker reflects that reality instead of pretending
    // the user has a choice.
    const [aiMouseMicName, setAiMouseMicName] = useState<string | null>(null);

    const probeAiMouseMic = React.useCallback(async () => {
      try {
        const name = await invoke<string | null>(
          "get_ai_mouse_microphone_name",
        );
        setAiMouseMicName(name ?? null);
      } catch {
        setAiMouseMicName(null);
      }
    }, []);

    useEffect(() => {
      probeAiMouseMic();
    }, [probeAiMouseMic, audioDevices]);

    const aiMouseOnline = aiMouseMicName !== null;

    const selectedMicrophone = aiMouseOnline
      ? aiMouseMicName!
      : getSetting("selected_microphone") === "default"
        ? "Default"
        : getSetting("selected_microphone") || "Default";

    const handleMicrophoneSelect = async (deviceName: string) => {
      if (aiMouseOnline) return; // locked
      await updateSetting("selected_microphone", deviceName);
    };

    const handleReset = async () => {
      if (aiMouseOnline) return;
      await resetSetting("selected_microphone");
    };

    const microphoneOptions = aiMouseOnline
      ? [
          {
            value: aiMouseMicName!,
            label: t("settings.sound.microphone.aiMouseLabel", {
              name: aiMouseMicName,
            }),
          },
        ]
      : audioDevices.map((device) => ({
          value: device.name,
          label: device.name,
        }));

    const helperText = aiMouseOnline
      ? t("settings.sound.microphone.aiMouseLocked", { name: aiMouseMicName })
      : audioDevices.length === 0
        ? t("settings.sound.microphone.aiMouseMissing")
        : null;

    return (
      <SettingContainer
        title={t("settings.sound.microphone.title")}
        description={t("settings.sound.microphone.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="flex flex-col space-y-1">
          <div className="flex items-center space-x-1">
            <Dropdown
              options={microphoneOptions}
              selectedValue={selectedMicrophone}
              onSelect={handleMicrophoneSelect}
              placeholder={
                isLoading || microphoneOptions.length === 0
                  ? t("settings.sound.microphone.loading")
                  : t("settings.sound.microphone.placeholder")
              }
              disabled={
                aiMouseOnline ||
                isUpdating("selected_microphone") ||
                isLoading ||
                microphoneOptions.length === 0
              }
              onRefresh={async () => {
                await refreshAudioDevices();
                await probeAiMouseMic();
              }}
            />
            <ResetButton
              onClick={handleReset}
              disabled={
                aiMouseOnline ||
                isUpdating("selected_microphone") ||
                isLoading
              }
            />
          </div>
          {helperText && (
            <span className="text-xs text-mid-gray">{helperText}</span>
          )}
        </div>
      </SettingContainer>
    );
  },
);
