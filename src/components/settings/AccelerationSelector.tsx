import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { SettingContainer } from "../ui/SettingContainer";
import { Dropdown, type DropdownOption } from "../ui/Dropdown";
import { useSettings } from "../../hooks/useSettings";
import { commands } from "@/bindings";

const WHISPER_LABELS: Record<string, string> = {
  auto: "Auto",
  cpu: "CPU",
  gpu: "GPU",
};

const ORT_LABELS: Record<string, string> = {
  auto: "Auto",
  cpu: "CPU",
  cuda: "CUDA",
  directml: "DirectML",
  rocm: "ROCm",
};

interface AccelerationSelectorProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const AccelerationSelector: React.FC<AccelerationSelectorProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();

  const [whisperOptions, setWhisperOptions] = useState<DropdownOption[]>([]);
  const [ortOptions, setOrtOptions] = useState<DropdownOption[]>([]);

  useEffect(() => {
    commands.getAvailableAccelerators().then((available) => {
      setWhisperOptions(
        available.whisper.map((v) => ({
          value: v,
          label: WHISPER_LABELS[v] ?? v,
        })),
      );
      // Always include "auto" for ORT even though available() only returns compiled-in backends
      const ortVals = available.ort.includes("auto")
        ? available.ort
        : ["auto", ...available.ort];
      setOrtOptions(
        ortVals.map((v) => ({
          value: v,
          label: ORT_LABELS[v] ?? v,
        })),
      );
    });
  }, []);

  const currentWhisper = getSetting("whisper_accelerator") ?? "auto";
  const currentOrt = getSetting("ort_accelerator") ?? "auto";

  // Map between settings enum format (direct_ml) and transcribe-rs format (directml)
  const ortSettingToValue = (setting: string) =>
    setting === "direct_ml" ? "directml" : setting;
  const ortValueToSetting = (value: string) =>
    value === "directml" ? "direct_ml" : value;

  return (
    <>
      <SettingContainer
        title={t("settings.advanced.acceleration.whisper.title")}
        description={t("settings.advanced.acceleration.whisper.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
        layout="horizontal"
      >
        <Dropdown
          options={whisperOptions}
          selectedValue={currentWhisper}
          onSelect={(value) =>
            updateSetting("whisper_accelerator", value as any)
          }
          disabled={isUpdating("whisper_accelerator")}
        />
      </SettingContainer>
      {ortOptions.length > 2 && (
        <SettingContainer
          title={t("settings.advanced.acceleration.ort.title")}
          description={t("settings.advanced.acceleration.ort.description")}
          descriptionMode={descriptionMode}
          grouped={grouped}
          layout="horizontal"
        >
          <Dropdown
            options={ortOptions}
            selectedValue={ortSettingToValue(currentOrt)}
            onSelect={(value) =>
              updateSetting("ort_accelerator", ortValueToSetting(value) as any)
            }
            disabled={isUpdating("ort_accelerator")}
          />
        </SettingContainer>
      )}
    </>
  );
};
