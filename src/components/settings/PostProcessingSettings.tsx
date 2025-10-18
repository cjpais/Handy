import React from "react";
import { SettingsGroup } from "../ui/SettingsGroup";
import { OpenRouterConfiguration } from "./OpenRouterConfiguration";
import { PromptsConfiguration } from "./PromptsConfiguration";

export const PostProcessingSettings: React.FC = () => {
  return (
    <div className="space-y-8 w-full">
      <SettingsGroup title="OpenRouter">
        <OpenRouterConfiguration />
      </SettingsGroup>
      
      <SettingsGroup title="Prompt">
        <PromptsConfiguration />
      </SettingsGroup>
    </div>
  );
};
