import React, { useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { useSettings } from "../../hooks/useSettings";
import { ModelUnloadTimeout } from "../../lib/types";
import { Dropdown } from "../ui/Dropdown";
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

  const handleChange = async (event: React.ChangeEvent<HTMLSelectElement>) => {
    const newTimeout = event.target.value as ModelUnloadTimeout;

    try {
      await invoke("set_model_unload_timeout", { timeout: newTimeout });
      updateSetting("model_unload_timeout", newTimeout);
    } catch (error) {
      console.error("Failed to update model unload timeout:", error);
    }
  };

  const currentValue = getSetting("model_unload_timeout") ?? "never";

  const options = useMemo(() => {
    const timeoutOptions = [
      {
        value: "never" as ModelUnloadTimeout,
        label: t("settings.debug.model_unload_timeout.options.never"),
      },
      {
        value: "immediately" as ModelUnloadTimeout,
        label: t("settings.debug.model_unload_timeout.options.immediately"),
      },
      {
        value: "min2" as ModelUnloadTimeout,
        label: t("settings.debug.model_unload_timeout.options.min2"),
      },
      {
        value: "min5" as ModelUnloadTimeout,
        label: t("settings.debug.model_unload_timeout.options.min5"),
      },
      {
        value: "min10" as ModelUnloadTimeout,
        label: t("settings.debug.model_unload_timeout.options.min10"),
      },
      {
        value: "min15" as ModelUnloadTimeout,
        label: t("settings.debug.model_unload_timeout.options.min15"),
      },
      {
        value: "hour1" as ModelUnloadTimeout,
        label: t("settings.debug.model_unload_timeout.options.hour1"),
      },
    ];

    if (settings?.debug_mode) {
      return [
        ...timeoutOptions,
        {
          value: "sec5" as ModelUnloadTimeout,
          label: t("settings.debug.model_unload_timeout.options.sec5"),
        },
      ];
    }

    return timeoutOptions;
  }, [settings?.debug_mode, t]);

  return (
    <SettingContainer
      title={t("settings.debug.model_unload_timeout.title")}
      description={t("settings.debug.model_unload_timeout.description")}
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
