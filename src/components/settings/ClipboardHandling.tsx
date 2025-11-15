import React from "react";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import type { ClipboardHandling } from "@/bindings";

interface ClipboardHandlingProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

const clipboardHandlingOptions = [
  { value: "dont_modify", label: "Don't Modify Clipboard" },
  { value: "copy_to_clipboard", label: "Copy to Clipboard" },
];

export const ClipboardHandlingSetting: React.FC<ClipboardHandlingProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const selectedHandling = (getSetting("clipboard_handling") ||
      "dont_modify") as ClipboardHandling;

    return (
      <SettingContainer
        title="Clipboard Handling"
        description="Don't Modify Clipboard preserves your current clipboard contents after transcription. Copy to Clipboard leaves the transcription result in your clipboard after pasting."
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
