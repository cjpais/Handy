import React, { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import type { ClipboardHandling } from "../../lib/types";

interface ClipboardHandlingProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const ClipboardHandlingSetting: React.FC<ClipboardHandlingProps> = React.memo(({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();

  const selectedHandling = (getSetting("clipboard_handling") ||
    "dont_modify") as ClipboardHandling;

  const clipboardHandlingOptions = useMemo(
    () => [
      {
        value: "dont_modify",
        label: t("settings.debug.clipboard_handling.options.dont_modify"),
      },
      {
        value: "copy_to_clipboard",
        label: t("settings.debug.clipboard_handling.options.copy_to_clipboard"),
      },
    ],
    [t],
  );

  return (
    <SettingContainer
      title={t("settings.debug.clipboard_handling.title")}
      description={t("settings.debug.clipboard_handling.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <Dropdown
        options={clipboardHandlingOptions}
        selectedValue={selectedHandling}
        onSelect={(value) =>
          updateSetting("clipboard_handling", value as ClipboardHandling)
        }
        disabled={isUpdating("clipboard_handling")}
      />
    </SettingContainer>
  );
});
