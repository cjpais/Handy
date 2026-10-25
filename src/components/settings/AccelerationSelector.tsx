import { type FC, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import { SettingContainer } from "../ui/SettingContainer";
import { Dropdown, type DropdownOption } from "../ui/Dropdown";
import { useSettings } from "../../hooks/useSettings";
import { commands } from "@/bindings";
import type {
  AvailableAccelerators,
  TranscribeAcceleratorSetting,
} from "@/bindings";
import type { ModelStateEvent } from "@/lib/types/events";

interface AccelerationSelectorProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

/**
 * transcribe.cpp dropdown encodes accelerator + device in a single value:
 *   "auto"       → accelerator=auto, gpu_device=null
 *   "cpu"        → accelerator=cpu,  gpu_device=null
 *   "gpu:<id>"   → accelerator=gpu, stable opaque device identity
 */
function encodeTranscribeValue(
  accelerator: TranscribeAcceleratorSetting,
  gpuDevice: string | null,
): string {
  if (accelerator === "cpu") return "cpu";
  if (accelerator === "gpu" && gpuDevice !== null) return `gpu:${gpuDevice}`;
  return "auto";
}

function decodeTranscribeValue(value: string): {
  accelerator: TranscribeAcceleratorSetting;
  gpuDevice: string | null;
} {
  if (value === "cpu") return { accelerator: "cpu", gpuDevice: null };
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

  const currentAccelerator = getSetting("transcribe_accelerator") ?? "auto";
  const currentGpuDevice = getSetting("transcribe_gpu_device") ?? null;
  const currentTranscribe = encodeTranscribeValue(
    currentAccelerator as TranscribeAcceleratorSetting,
    currentGpuDevice as string | null,
  );
  const [available, setAvailable] = useState<AvailableAccelerators | null>(
    null,
  );
  const refreshRef = useRef<() => void>(() => {});

  useEffect(() => {
    let disposed = false;
    let requestId = 0;
    const refresh = async () => {
      const request = ++requestId;
      try {
        const result = await commands.getAvailableAccelerators();
        if (!disposed && request === requestId) setAvailable(result);
      } catch (error) {
        if (!disposed)
          console.error("Failed to refresh transcription devices:", error);
      }
    };
    refreshRef.current = () => {
      void refresh();
    };
    // Binding changes precede successful inference. Refresh immediately so
    // CPU is not displayed while a newly bound GPU is already in use.
    const deviceChanged = listen(
      "transcription-device-changed",
      () => void refresh(),
    );
    const modelState = listen<ModelStateEvent>(
      "model-state-changed",
      (event) => {
        if (
          ["loading_completed", "unloaded", "loading_failed"].includes(
            event.payload.event_type,
          )
        ) {
          void refresh();
        }
      },
    );
    void Promise.all([deviceChanged, modelState])
      .then(() => refresh())
      .catch((error) => {
        console.error(
          "Failed to listen for transcription device changes:",
          error,
        );
      });
    return () => {
      disposed = true;
      refreshRef.current = () => {};
      for (const subscription of [deviceChanged, modelState]) {
        void subscription.then((unlisten) => unlisten()).catch(console.error);
      }
    };
  }, [currentAccelerator, currentGpuDevice]);

  const transcribeOptions: DropdownOption[] = [];
  if (available?.transcribe.includes("auto")) {
    transcribeOptions.push({
      value: "auto",
      label: t("settings.advanced.acceleration.gpuDevice.auto"),
    });
  }
  for (const dev of available?.gpu_devices ?? []) {
    const name = dev.name || t("settings.advanced.acceleration.preferredGpu");
    const vramLabel =
      dev.total_vram_mb >= 1024
        ? `${(dev.total_vram_mb / 1024).toFixed(1)} GB`
        : `${dev.total_vram_mb} MB`;
    transcribeOptions.push({
      value: `gpu:${dev.id}`,
      label: dev.available
        ? `${name} (${vramLabel})`
        : t("settings.advanced.acceleration.unavailableDevice", {
            device: name,
          }),
    });
  }
  if (available?.transcribe.includes("cpu")) {
    transcribeOptions.push({ value: "cpu", label: "CPU" });
  }
  // Auto is a policy, not an extra physical device. Unavailable saved GPUs
  // also do not count as alternatives when deciding whether to explain it.
  const usableDeviceCount =
    (available?.gpu_devices.filter((device) => device.available).length ?? 0) +
    Number(available?.transcribe.includes("cpu"));
  const selectedGpuName =
    available?.gpu_devices.find((device) => device.id === currentGpuDevice)
      ?.name || t("settings.advanced.acceleration.preferredGpu");
  const activeDevice = available?.active_device ?? null;

  const handleTranscribeChange = async (value: string) => {
    const { accelerator, gpuDevice } = decodeTranscribeValue(value);
    // Save the device first to avoid `gpu + null` being normalized to Auto.
    await updateSetting("transcribe_gpu_device", gpuDevice);
    await updateSetting("transcribe_accelerator", accelerator);
  };

  // The dropdown is the saved preference. Explain Auto only when multiple
  // usable devices exist, and hide redundant status for a matching explicit
  // choice. A manual switch is neutral until applied; amber is reserved for
  // actual fallback, even when CPU is the only remaining usable device.
  const showActiveDevice =
    activeDevice !== null &&
    (activeDevice.is_pending ||
      activeDevice.is_fallback ||
      (currentTranscribe === "auto" && usableDeviceCount > 1));

  let deviceStatus = "";
  if (activeDevice?.is_pending) {
    deviceStatus = t("settings.advanced.acceleration.pendingSwitch", {
      device: currentTranscribe === "cpu" ? "CPU" : selectedGpuName,
    });
  } else if (activeDevice) {
    deviceStatus = t(
      activeDevice.is_fallback
        ? "settings.advanced.acceleration.currentlyUsingFallback"
        : "settings.advanced.acceleration.currentlyUsing",
      {
        device: activeDevice.on_gpu ? activeDevice.name : t("errors.backupCpu"),
      },
    );
  }

  return (
    <SettingContainer
      title={t("settings.advanced.acceleration.transcribe.title")}
      description={t("settings.advanced.acceleration.transcribe.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
      layout="horizontal"
    >
      <div className="flex flex-col items-end gap-1">
        <Dropdown
          options={transcribeOptions}
          selectedValue={currentTranscribe}
          onSelect={handleTranscribeChange}
          onOpen={() => refreshRef.current()}
          disabled={
            isUpdating("transcribe_accelerator") ||
            isUpdating("transcribe_gpu_device")
          }
        />
        {showActiveDevice && activeDevice && (
          <p
            className={`max-w-sm text-right text-xs ${
              activeDevice.is_fallback
                ? "text-amber-600 dark:text-amber-400"
                : "text-text/60"
            }`}
            aria-live="polite"
          >
            {deviceStatus}
          </p>
        )}
      </div>
    </SettingContainer>
  );
};
