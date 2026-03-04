import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useSettings } from "../../hooks/useSettings";
import { commands, type GpuProvider } from "@/bindings";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";

interface GpuProviderProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

const PROVIDER_LABEL_KEYS: Record<string, string> = {
  auto: "settings.advanced.gpuProvider.options.auto",
  cpu: "settings.advanced.gpuProvider.options.cpu",
  directml: "settings.advanced.gpuProvider.options.directml",
  cuda: "settings.advanced.gpuProvider.options.cuda",
  coreml: "settings.advanced.gpuProvider.options.coreml",
  webgpu: "settings.advanced.gpuProvider.options.webgpu",
};

export const GpuProviderSetting: React.FC<GpuProviderProps> = ({
  descriptionMode = "inline",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting } = useSettings();
  const [availableProviders, setAvailableProviders] = useState<
    GpuProvider[] | null
  >(null);
  const [isChanging, setIsChanging] = useState(false);

  useEffect(() => {
    commands.getAvailableGpuProviders().then(setAvailableProviders);
  }, []);

  const options = (availableProviders ?? []).map((p) => ({
    value: p,
    label: t(PROVIDER_LABEL_KEYS[p] ?? p),
  }));

  const currentValue = (getSetting("gpu_provider") as GpuProvider) ?? "auto";

  const handleChange = async (value: string) => {
    if (isChanging) return;
    setIsChanging(true);
    try {
      const result = await commands.changeGpuProviderSetting(
        value as GpuProvider,
      );
      if (result.status === "ok") {
        updateSetting("gpu_provider", value);
      } else {
        console.error("Failed to change GPU provider:", result.error);
        toast.error(t("settings.advanced.gpuProvider.busyError"));
      }
    } catch (error) {
      console.error("Failed to change GPU provider:", error);
      toast.error(String(error));
    } finally {
      setIsChanging(false);
    }
  };

  // Only show if there's more than just auto + cpu (i.e. a GPU EP is compiled in)
  if (availableProviders === null || availableProviders.length <= 2) {
    return null;
  }

  return (
    <SettingContainer
      title={t("settings.advanced.gpuProvider.title")}
      description={t("settings.advanced.gpuProvider.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
      disabled={isChanging}
    >
      <Dropdown
        options={options}
        selectedValue={currentValue}
        onSelect={handleChange}
        disabled={isChanging}
      />
    </SettingContainer>
  );
};
