import React from "react";
import { useTranslation } from "react-i18next";
import { Slider } from "../../ui/Slider";
import { useSettings } from "../../../hooks/useSettings";
import { useOsType } from "../../../hooks/useOsType";

interface ClipboardRestoreDelayProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

/**
 * How long the transcript stays on the clipboard before the previous
 * clipboard is restored. macOS always restores this way; Windows only with
 * reliable paste.
 */
export const ClipboardRestoreDelay: React.FC<ClipboardRestoreDelayProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { settings, updateSetting, resetSetting, isUpdating } = useSettings();
  const osType = useOsType();

  // Linux restores inside the paste itself, after "Paste Delay (After)".
  if (osType !== "macos" && osType !== "windows") {
    return null;
  }

  return (
    <Slider
      value={settings?.clipboard_restore_delay_ms ?? 1000}
      onChange={(value) => updateSetting("clipboard_restore_delay_ms", value)}
      onReset={() => resetSetting("clipboard_restore_delay_ms")}
      isResetting={isUpdating("clipboard_restore_delay_ms")}
      disabled={osType === "windows" && !settings?.reliable_paste}
      min={100}
      max={5000}
      step={100}
      label={t("settings.debug.clipboardRestoreDelay.title")}
      description={t("settings.debug.clipboardRestoreDelay.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
      formatValue={(v) => `${v}ms`}
    />
  );
};
