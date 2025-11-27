import React from "react";
import { useSettings } from "../../hooks/useSettings";
import { Input } from "../ui/Input";
import { SettingContainer } from "../ui/SettingContainer";

interface HistoryLimitProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const HistoryLimit: React.FC<HistoryLimitProps> = ({
  descriptionMode = "inline",
  grouped = false,
}) => {
  const { getSetting, updateSetting, isUpdating } = useSettings();

  const historyLimit = Number(getSetting("history_limit") ?? "5");

  const handleChange = async (event: React.ChangeEvent<HTMLInputElement>) => {
    const value = parseInt(event.target.value, 10);
    if (!isNaN(value) && value >= 0) {
      updateSetting("history_limit", value.toString());
    }
  };

  return (
    <SettingContainer
      title="History Limit"
      description="Maximum number of transcription entries to keep in history"
      descriptionMode={descriptionMode}
      grouped={grouped}
      layout="horizontal"
    >
      <div className="flex items-center space-x-2">
        <Input
          type="number"
          min="0"
          max="1000"
          value={historyLimit}
          onChange={handleChange}
          disabled={isUpdating("history_limit")}
          className="w-20"
        />
        <span className="text-sm text-text">entries</span>
      </div>
    </SettingContainer>
  );
};
