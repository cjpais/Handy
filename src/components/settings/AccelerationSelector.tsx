import { type FC, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { SettingContainer } from "../ui/SettingContainer";
import { Dropdown, type DropdownOption } from "../ui/Dropdown";
import { useSettings } from "../../hooks/useSettings";
import { commands } from "@/bindings";
import type {
  TranscribeAcceleratorSetting,
  OrtAcceleratorSetting,
} from "@/bindings";

const ORT_LABELS: Record<OrtAcceleratorSetting, string> = {
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

/**
 * transcribe.cpp dropdown encodes accelerator + device in a single value:
 *   "auto"       → accelerator=auto, gpu_device=null
 *   "cpu"        → accelerator=cpu,  gpu_device=null
 *   "gpu"        → accelerator=gpu, auto GPU selection
 *   "gpu:<id>"   → accelerator=gpu, stable opaque device identity
 */
function encodeTranscribeValue(
  accelerator: TranscribeAcceleratorSetting,
  gpuDevice: string | null,
): string {
  if (accelerator === "cpu") return "cpu";
  if (accelerator === "gpu")
    return gpuDevice === null ? "gpu" : `gpu:${gpuDevice}`;
  return "auto";
}

function decodeTranscribeValue(value: string): {
  accelerator: TranscribeAcceleratorSetting;
  gpuDevice: string | null;
} {
  if (value === "cpu") return { accelerator: "cpu", gpuDevice: null };
  if (value === "gpu") return { accelerator: "gpu", gpuDevice: null };
  if (value.startsWith("gpu:")) {
    return { accelerator: "gpu", gpuDevice: value.slice(4) };
  }
  return { accelerator: "auto", gpuDevice: null };
}

export const AccelerationSelector: FC<AccelerationSelectorProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();

  const [transcribeOptions, setTranscribeOptions] = useState<DropdownOption[]>(
    [],
  );
  const [ortOptions, setOrtOptions] = useState<DropdownOption[]>([]);

  useEffect(() => {
    commands.getAvailableAccelerators().then((available) => {
      // Build combined transcribe.cpp options: Auto, [GPU devices...], CPU
      const opts: DropdownOption[] = [];
      if (available.transcribe.includes("auto")) {
        opts.push({
          value: "auto",
          label: t("settings.advanced.acceleration.gpuDevice.auto"),
        });
      }

      if (available.transcribe.includes("gpu")) {
        opts.push({ value: "gpu", label: "GPU" });
        for (const dev of available.gpu_devices) {
          const vramLabel =
            dev.total_vram_mb >= 1024
              ? `${(dev.total_vram_mb / 1024).toFixed(1)} GB`
              : `${dev.total_vram_mb} MB`;
          opts.push({
            value: `gpu:${dev.id}`,
            label: `${dev.name} (${vramLabel})`,
          });
        }
      }

      if (available.transcribe.includes("cpu")) {
        opts.push({ value: "cpu", label: "CPU" });
      }
      setTranscribeOptions(opts);

      // ORT options (unchanged)
      const ortVals = available.ort.includes("auto")
        ? available.ort
        : ["auto", ...available.ort];
      setOrtOptions(
        ortVals.map((v) => ({
          value: v,
          label: ORT_LABELS[v as OrtAcceleratorSetting] ?? v,
        })),
      );
    });
  }, [t]);

  const currentAccelerator = getSetting("transcribe_accelerator") ?? "auto";
  const currentGpuDevice = getSetting("transcribe_gpu_device") ?? null;
  const currentTranscribe = encodeTranscribeValue(
    currentAccelerator as TranscribeAcceleratorSetting,
    currentGpuDevice as string | null,
  );
  const displayedTranscribe = transcribeOptions.some(
    (option) => option.value === currentTranscribe,
  )
    ? currentTranscribe
    : currentAccelerator === "gpu" &&
        transcribeOptions.some((option) => option.value === "gpu")
      ? "gpu"
      : (transcribeOptions[0]?.value ?? null);
  const currentOrt = getSetting("ort_accelerator") ?? "auto";

  const handleTranscribeChange = async (value: string) => {
    const { accelerator, gpuDevice } = decodeTranscribeValue(value);
    await updateSetting("transcribe_accelerator", accelerator);
    await updateSetting("transcribe_gpu_device", gpuDevice);
  };

  return (
    <>
      <SettingContainer
        title={t("settings.advanced.acceleration.transcribe.title")}
        description={t("settings.advanced.acceleration.transcribe.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
        layout="horizontal"
      >
        <Dropdown
          options={transcribeOptions}
          selectedValue={displayedTranscribe}
          onSelect={handleTranscribeChange}
          disabled={
            isUpdating("transcribe_accelerator") ||
            isUpdating("transcribe_gpu_device")
          }
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
            selectedValue={currentOrt}
            onSelect={(value) =>
              updateSetting("ort_accelerator", value as OrtAcceleratorSetting)
            }
            disabled={isUpdating("ort_accelerator")}
          />
        </SettingContainer>
      )}
    </>
  );
};
