import React from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import type { ShortcutActivation } from "@/bindings";

interface ShortcutActivationProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const ShortcutActivationSetting: React.FC<ShortcutActivationProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const options = [
      {
        value: "hold_or_toggle",
        label: t("settings.general.shortcutActivation.options.holdOrToggle"),
      },
      {
        value: "push_to_talk",
        label: t("settings.general.shortcutActivation.options.pushToTalk"),
      },
      {
        value: "toggle",
        label: t("settings.general.shortcutActivation.options.toggle"),
      },
    ];

    const selected = (getSetting("shortcut_activation") ||
      "hold_or_toggle") as ShortcutActivation;

    return (
      <SettingContainer
        title={t("settings.general.shortcutActivation.title")}
        description={t(
          `settings.general.shortcutActivation.descriptions.${selected}`,
        )}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <Dropdown
          options={options}
          selectedValue={selected}
          onSelect={(value) =>
            updateSetting("shortcut_activation", value as ShortcutActivation)
          }
          disabled={isUpdating("shortcut_activation")}
        />
      </SettingContainer>
    );
  });
