import React from "react";
import { WordCorrectionThreshold } from "./debug/WordCorrectionThreshold";
import { AppDataDirectory } from "./AppDataDirectory";
import { SettingsGroup } from "../ui/SettingsGroup";
import { HistoryLimit } from "./HistoryLimit";

export const DebugSettings: React.FC = () => {
  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title="Debug">
        <WordCorrectionThreshold descriptionMode="tooltip" grouped={true} />
        <AppDataDirectory descriptionMode="tooltip" grouped={true} />
        <HistoryLimit descriptionMode="tooltip" grouped={true} />
      </SettingsGroup>
    </div>
  );
};
