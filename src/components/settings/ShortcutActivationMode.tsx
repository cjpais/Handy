import React, { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { SettingContainer } from "../ui/SettingContainer";
import { Dropdown, type DropdownOption } from "../ui/Dropdown";
import { useSettings } from "../../hooks/useSettings";

type ShortcutActivationModeValue = "tap" | "push_to_talk" | "double_tap";

interface ShortcutActivationModeProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

function getCurrentMode(
  pushToTalk: boolean,
  doubleTapActivation: boolean,
): ShortcutActivationModeValue {
  if (pushToTalk) return "push_to_talk";
  if (doubleTapActivation) return "double_tap";
  return "tap";
}

export const ShortcutActivationMode: React.FC<ShortcutActivationModeProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const pushToTalk = getSetting("push_to_talk") || false;
    const doubleTapActivation = getSetting("double_tap_activation") || false;
    const selectedMode = getCurrentMode(pushToTalk, doubleTapActivation);

    const options: DropdownOption[] = useMemo(
      () => [
        {
          value: "tap",
          label: t("settings.general.shortcutActivation.options.tap"),
        },
        {
          value: "push_to_talk",
          label: t("settings.general.shortcutActivation.options.pushToTalk"),
        },
        {
          value: "double_tap",
          label: t("settings.general.shortcutActivation.options.doubleTap"),
        },
      ],
      [t],
    );

    const description = t(
      `settings.general.shortcutActivation.descriptions.${selectedMode}`,
    );

    const handleSelect = async (value: string) => {
      const mode = value as ShortcutActivationModeValue;
      if (mode === selectedMode) return;

      await updateSetting("push_to_talk", mode === "push_to_talk");
      await updateSetting("double_tap_activation", mode === "double_tap");
    };

    return (
      <SettingContainer
        title={t("settings.general.shortcutActivation.label")}
        description={description}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <Dropdown
          options={options}
          selectedValue={selectedMode}
          onSelect={handleSelect}
          disabled={
            isUpdating("push_to_talk") || isUpdating("double_tap_activation")
          }
        />
      </SettingContainer>
    );
  });

ShortcutActivationMode.displayName = "ShortcutActivationMode";
