import React from "react";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import type { PasteMethod } from "../../lib/types";

interface PasteMethodProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

const pasteMethodOptions = [
  { value: "ctrl_v", label: "Clipboard (Ctrl+V)" },
  { value: "direct", label: "Direct" },
];

export const PasteMethodSetting: React.FC<PasteMethodProps> = React.memo(({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { getSetting, updateSetting, isUpdating } = useSettings();

  const selectedMethod = (getSetting("paste_method") ||
    "ctrl_v") as PasteMethod;

  return (
    <SettingContainer
      title="Paste Method"
      description="Clipboard (Ctrl+V) simulates Ctrl/Cmd+V keystrokes to paste from your clipboard. Direct tries to use system input methods if possible, otherwise inputs keystrokes one by one into the text field."
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <Dropdown
        options={pasteMethodOptions}
        selectedValue={selectedMethod}
        onSelect={(value) =>
          updateSetting("paste_method", value as PasteMethod)
        }
        disabled={isUpdating("paste_method")}
      />
    </SettingContainer>
  );
});
