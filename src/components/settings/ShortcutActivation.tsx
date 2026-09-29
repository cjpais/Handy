import React from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useShortcutSetting } from "../../hooks/useShortcutSetting";
import type { ShortcutActivation } from "@/bindings";

interface ShortcutActivationProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
  /** Edit this transcribe shortcut's own mode instead of the global one. */
  bindingId?: string;
}

export const ShortcutActivationSetting: React.FC<ShortcutActivationProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false, bindingId }) => {
    const { t } = useTranslation();
    const activation = useShortcutSetting(bindingId, "shortcut_activation");

    const options = [
      {
        value: "hold_or_toggle",
        label: t("settings.general.shortcutActivation.options.holdOrToggle"),
        description: t(
          "settings.general.shortcutActivation.descriptions.hold_or_toggle",
        ),
      },
      {
        value: "push_to_talk",
        label: t("settings.general.shortcutActivation.options.pushToTalk"),
        description: t(
          "settings.general.shortcutActivation.descriptions.push_to_talk",
        ),
      },
      {
        value: "toggle",
        label: t("settings.general.shortcutActivation.options.toggle"),
        description: t(
          "settings.general.shortcutActivation.descriptions.toggle",
        ),
      },
    ];

    const selected = (activation.value ||
      "hold_or_toggle") as ShortcutActivation;

    return (
      <SettingContainer
        title={t("settings.general.shortcutActivation.title")}
        description={t("settings.general.shortcutActivation.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <Dropdown
          options={options}
          menuClassName="right-0 w-80 max-w-[calc(100vw-2rem)]"
          selectedValue={selected}
          onSelect={(value) => activation.update(value as ShortcutActivation)}
          disabled={activation.isUpdating}
        />
      </SettingContainer>
    );
  });
