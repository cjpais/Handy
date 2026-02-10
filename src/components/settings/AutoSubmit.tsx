import React from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import { useOsType } from "../../hooks/useOsType";
import type { AutoSubmitKey } from "@/bindings";

interface AutoSubmitProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

type AutoSubmitOptionValue = AutoSubmitKey | "off";

export const AutoSubmit: React.FC<AutoSubmitProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const osType = useOsType();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("auto_submit") ?? false;
    const selectedKey = (getSetting("auto_submit_key") ||
      "enter") as AutoSubmitKey;
    const selectedValue: AutoSubmitOptionValue = enabled ? selectedKey : "off";
    const submitWithMetaLabel =
      osType === "macos"
        ? t("settings.advanced.autoSubmit.options.cmdEnter")
        : t("settings.advanced.autoSubmit.options.superEnter");

    const autoSubmitOptions = [
      {
        value: "off",
        label: t("settings.advanced.autoSubmit.options.off"),
      },
      {
        value: "enter",
        label: t("settings.advanced.autoSubmit.options.enter"),
      },
      {
        value: "ctrl_enter",
        label: t("settings.advanced.autoSubmit.options.ctrlEnter"),
      },
      {
        value: "cmd_enter",
        label: submitWithMetaLabel,
      },
    ];

    const handleAutoSubmitSelect = async (value: string) => {
      const selected = value as AutoSubmitOptionValue;

      if (selected === "off") {
        await updateSetting("auto_submit", false);
        return;
      }

      await updateSetting("auto_submit_key", selected as AutoSubmitKey);
      if (!enabled) {
        await updateSetting("auto_submit", true);
      }
    };

    return (
      <SettingContainer
        title={t("settings.advanced.autoSubmit.title")}
        description={t("settings.advanced.autoSubmit.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <Dropdown
          options={autoSubmitOptions}
          selectedValue={selectedValue}
          onSelect={handleAutoSubmitSelect}
          disabled={isUpdating("auto_submit") || isUpdating("auto_submit_key")}
        />
      </SettingContainer>
    );
  },
);
