import React from "react";
import { InitialPrompt } from "./InitialPrompt";
import { SettingsGroup } from "../ui/SettingsGroup";

export const InitialPromptPage: React.FC = () => {
  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title="Initial Prompt">
        <InitialPrompt descriptionMode="tooltip" grouped />
      </SettingsGroup>
    </div>
  );
};