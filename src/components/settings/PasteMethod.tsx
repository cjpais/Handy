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
  const baseOptions = [
    { value: "ctrl_v", label: "Clipboard (Ctrl+V)" },
    { value: "direct", label: "Direct" },
    { value: "none", label: "None" },
  ];

  // Add Shift+Insert option for Windows and Linux only
  if (osType === "windows" || osType === "linux") {
    baseOptions.push({
      value: "shift_insert",
      label: "Clipboard (Shift+Insert)",
    });
  }

  return baseOptions;
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
        description="Ctrl+V: 'pastes' via ctrl+v. Direct: type text directly. Shift+Insert: pastes via shift+insert. None: skip paste; just update history/clipboard."
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
