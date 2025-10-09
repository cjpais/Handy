import React from "react";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import type { AutoSubmitKey } from "../../lib/types";

interface AutoSubmitProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

const autoSubmitKeyOptions = [
  { value: "enter", label: "Enter" },
  { value: "ctrl_enter", label: "Ctrl+Enter" },
  { value: "cmd_enter", label: "Cmd+Enter" },
];

export const AutoSubmit: React.FC<AutoSubmitProps> = React.memo(({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { getSetting, updateSetting, isUpdating } = useSettings();

  const autoSubmitEnabled = getSetting("auto_submit") || false;
  const selectedKey = (getSetting("auto_submit_key") || "enter") as AutoSubmitKey;

  return (
    <SettingContainer
      title="Auto Submit"
      description="Automatically submit transcription with configurable key"
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <div className="flex items-center gap-2">
        <Dropdown
          options={autoSubmitKeyOptions}
          selectedValue={selectedKey}
          onSelect={(value) => updateSetting("auto_submit_key", value as AutoSubmitKey)}
          disabled={!autoSubmitEnabled || isUpdating("auto_submit_key")}
        />
        <label className="inline-flex items-center cursor-pointer select-none">
          <input
            type="checkbox"
            value=""
            className="sr-only peer"
            checked={autoSubmitEnabled}
            disabled={isUpdating("auto_submit")}
            onChange={(e) => updateSetting("auto_submit", e.target.checked)}
          />
          <div className="relative w-11 h-6 bg-mid-gray/20 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-logo-primary rounded-full peer peer-checked:after:translate-x-full rtl:peer-checked:after:-translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:start-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-background-ui peer-disabled:opacity-50"></div>
        </label>
      </div>
    </SettingContainer>
  );
});
