import React, { useMemo } from "react";
import { useSettings } from "../../hooks/useSettings";
import { commands, type ModelUnloadTimeout } from "@/bindings";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";

interface ModelUnloadTimeoutProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

const timeoutOptions = [
  { value: "never" as ModelUnloadTimeout, label: "Never" },
  { value: "immediately" as ModelUnloadTimeout, label: "Immediately" },
  { value: "min2" as ModelUnloadTimeout, label: "After 2 minutes" },
  { value: "min5" as ModelUnloadTimeout, label: "After 5 minutes" },
  { value: "min10" as ModelUnloadTimeout, label: "After 10 minutes" },
  { value: "min15" as ModelUnloadTimeout, label: "After 15 minutes" },
  { value: "hour1" as ModelUnloadTimeout, label: "After 1 hour" },
];

const debugTimeoutOptions = [
  ...timeoutOptions,
  { value: "sec5" as ModelUnloadTimeout, label: "After 5 seconds (Debug)" },
];

export const ModelUnloadTimeoutSetting: React.FC<ModelUnloadTimeoutProps> = ({
  descriptionMode = "inline",
  grouped = false,
}) => {
  const { settings, getSetting, updateSetting } = useSettings();

  const handleChange = async (event: React.ChangeEvent<HTMLSelectElement>) => {
    const newTimeout = event.target.value as ModelUnloadTimeout;

    try {
      await commands.setModelUnloadTimeout(newTimeout);
      updateSetting("model_unload_timeout", newTimeout);
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
      title="Unload Model"
      description="Automatically free GPU/CPU memory when the model hasn't been used for the specified time"
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <Dropdown
        options={options}
        selectedValue={currentValue}
        onSelect={(value) =>
          handleChange({
            target: { value },
          } as React.ChangeEvent<HTMLSelectElement>)
        }
        disabled={false}
      />
    </SettingContainer>
  );
};
