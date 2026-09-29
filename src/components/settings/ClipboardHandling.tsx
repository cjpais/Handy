import React from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useShortcutSetting } from "../../hooks/useShortcutSetting";
import type { ClipboardHandling } from "@/bindings";

interface ClipboardHandlingProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
  /** Edit this transcribe shortcut's own setting instead of the global one. */
  bindingId?: string;
}

export const ClipboardHandlingSetting: React.FC<ClipboardHandlingProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false, bindingId }) => {
    const { t } = useTranslation();
    const handling = useShortcutSetting(bindingId, "clipboard_handling");

    const clipboardHandlingOptions = [
      {
        value: "dont_modify",
        label: t("settings.advanced.clipboardHandling.options.dontModify"),
      },
      {
        value: "copy_to_clipboard",
        label: t("settings.advanced.clipboardHandling.options.copyToClipboard"),
      },
    ];

    const selectedHandling = (handling.value ||
      "dont_modify") as ClipboardHandling;

    return (
      <SettingContainer
        title={t("settings.advanced.clipboardHandling.title")}
        description={t("settings.advanced.clipboardHandling.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <Dropdown
          options={clipboardHandlingOptions}
          selectedValue={selectedHandling}
          onSelect={(value) => handling.update(value as ClipboardHandling)}
          disabled={handling.isUpdating}
        />
      </SettingContainer>
    );
  });
