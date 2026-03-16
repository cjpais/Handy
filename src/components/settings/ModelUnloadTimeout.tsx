import React, { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "../../hooks/useSettings";
import type { ModelUnloadTimeout } from "@/bindings";
import { Dropdown, type DropdownOption } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";

interface ModelUnloadTimeoutProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const ModelUnloadTimeoutSetting: React.FC<ModelUnloadTimeoutProps> = ({
  descriptionMode = "inline",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { settings, getSetting, updateSetting } = useSettings();

  const timeoutOptions: DropdownOption<ModelUnloadTimeout>[] = [
    {
      value: "never",
      label: t("settings.advanced.modelUnload.options.never"),
    },
    {
      value: "immediately",
      label: t("settings.advanced.modelUnload.options.immediately"),
    },
    {
      value: "min2",
      label: t("settings.advanced.modelUnload.options.min2"),
    },
    {
      value: "min5",
      label: t("settings.advanced.modelUnload.options.min5"),
    },
    {
      value: "min10",
      label: t("settings.advanced.modelUnload.options.min10"),
    },
    {
      value: "min15",
      label: t("settings.advanced.modelUnload.options.min15"),
    },
    {
      value: "hour1",
      label: t("settings.advanced.modelUnload.options.hour1"),
    },
  ];

  const debugTimeoutOptions: DropdownOption<ModelUnloadTimeout>[] = [
    ...timeoutOptions,
    {
      value: "sec5",
      label: t("settings.advanced.modelUnload.options.sec5"),
    },
  ];

  const handleChange = async (newTimeout: ModelUnloadTimeout) => {
    try {
      await updateSetting("model_unload_timeout", newTimeout);
    } catch (error) {
      console.error("Failed to update model unload timeout:", error);
    }
  };

  const currentValue = getSetting("model_unload_timeout") ?? "never";

  const options = useMemo(() => {
    return settings?.debug_mode === true ? debugTimeoutOptions : timeoutOptions;
  }, [settings]);

  return (
    <SettingContainer
      title={t("settings.advanced.modelUnload.title")}
      description={t("settings.advanced.modelUnload.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <Dropdown
        options={options}
        selectedValue={currentValue}
        onSelect={handleChange}
        disabled={false}
      />
    </SettingContainer>
  );
};
