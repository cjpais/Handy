import React, { useEffect, useState } from "react";
import { type as getOsType } from "@tauri-apps/plugin-os";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import type { PasteMethod } from "@/bindings";

interface PasteMethodProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

const getPasteMethodOptions = (osType: string) => {
  const mod = osType === "macos" ? "Cmd" : "Ctrl";

  const options = [
    { value: "ctrl_v", label: `Clipboard (${mod}+V)` },
    { value: "direct", label: "Direct" },
    { value: "none", label: "None" },
  ];

  // Add Shift+Insert and Ctrl+Shift+V options for Windows and Linux only
  if (osType === "windows" || osType === "linux") {
    options.push(
      { value: "ctrl_shift_v", label: "Clipboard (Ctrl+Shift+V)" },
      { value: "shift_insert", label: "Clipboard (Shift+Insert)" },
    );
  }

  return options;
};

export const PasteMethodSetting: React.FC<PasteMethodProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const [osType, setOsType] = useState<string>("unknown");

    useEffect(() => {
      setOsType(getOsType());
    }, []);

    const selectedMethod = (getSetting("paste_method") ||
      "ctrl_v") as PasteMethod;

    const pasteMethodOptions = getPasteMethodOptions(osType);

    return (
      <SettingContainer
        title="Paste Method"
        description="Choose how text is inserted. Direct: simulates typing via system input. None: skips paste, only updates history/clipboard."
        descriptionMode={descriptionMode}
        grouped={grouped}
        tooltipPosition="bottom"
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
  },
);
