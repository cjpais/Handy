import React from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useShortcutSetting } from "../../hooks/useShortcutSetting";
import { useOsType } from "../../hooks/useOsType";
import type { AutoSubmitKey } from "@/bindings";

interface AutoSubmitProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
  /** Edit this transcribe shortcut's own setting instead of the global one. */
  bindingId?: string;
}

type AutoSubmitOptionValue = AutoSubmitKey | "off";

export const AutoSubmit: React.FC<AutoSubmitProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false, bindingId }) => {
    const { t } = useTranslation();
    const osType = useOsType();
    const autoSubmit = useShortcutSetting(bindingId, "auto_submit");
    const autoSubmitKey = useShortcutSetting(bindingId, "auto_submit_key");

    const enabled = autoSubmit.value ?? false;
    const selectedKey = (autoSubmitKey.value || "enter") as AutoSubmitKey;
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
        await autoSubmit.update(false);
        return;
      }

      await autoSubmitKey.update(selected as AutoSubmitKey);
      if (!enabled) {
        await autoSubmit.update(true);
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
          disabled={autoSubmit.isUpdating || autoSubmitKey.isUpdating}
        />
      </SettingContainer>
    );
  },
);
