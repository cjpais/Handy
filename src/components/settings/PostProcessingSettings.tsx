import React from "react";
import { SettingsGroup } from "../ui/SettingsGroup";
import { PostProcessingSettingsApi } from "./PostProcessingSettingsApi";
import { PostProcessingSettingsPrompts } from "./PostProcessingSettingsPrompts";

export const PostProcessingSettings: React.FC = () => {
  return (
    <div className="space-y-8 w-full">
      <SettingsGroup title="API (OpenAI Compatible)">
        <PostProcessingSettingsApi />
      </SettingsGroup>
      
      <SettingsGroup title="Prompt">
        <PostProcessingSettingsPrompts />
      </SettingsGroup>
    </div>
  );
};
