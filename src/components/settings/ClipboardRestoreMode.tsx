import React from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import type { ClipboardRestoreMode } from "@/bindings";

interface ClipboardRestoreModeProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const ClipboardRestoreModeSetting: React.FC<ClipboardRestoreModeProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const clipboardRestoreModeOptions = [
      {
        value: "text_only",
        label: t("settings.advanced.clipboardRestoreMode.options.textOnly"),
      },
      {
        value: "all_formats",
        label: t("settings.advanced.clipboardRestoreMode.options.allFormats"),
      },
    ];

    const selectedMode = (getSetting("clipboard_restore_mode") ||
      "text_only") as ClipboardRestoreMode;

    return (
      <SettingContainer
        title={t("settings.advanced.clipboardRestoreMode.title")}
        description={t("settings.advanced.clipboardRestoreMode.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <Dropdown
          options={clipboardRestoreModeOptions}
          selectedValue={selectedMode}
          onSelect={(value) =>
            updateSetting(
              "clipboard_restore_mode",
              value as ClipboardRestoreMode,
            )
          }
          disabled={isUpdating("clipboard_restore_mode")}
        />
      </SettingContainer>
    );
  });
