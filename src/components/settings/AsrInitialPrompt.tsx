import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { SettingContainer } from "../ui/SettingContainer";
import { Textarea } from "../ui/Textarea";
import { useSettings } from "../../hooks/useSettings";

interface AsrInitialPromptProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const AsrInitialPrompt: React.FC<AsrInitialPromptProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("asr_prompt_enabled") || false;
    const prompt = getSetting("asr_initial_prompt") || "";

    return (
      <>
        <ToggleSwitch
          checked={enabled}
          onChange={(checked) => updateSetting("asr_prompt_enabled", checked)}
          isUpdating={isUpdating("asr_prompt_enabled")}
          label={t("settings.advanced.asrInitialPrompt.toggleLabel")}
          description={t(
            "settings.advanced.asrInitialPrompt.toggleDescription",
          )}
          descriptionMode={descriptionMode}
          grouped={grouped}
        />
        <SettingContainer
          title={t("settings.advanced.asrInitialPrompt.title")}
          description={t("settings.advanced.asrInitialPrompt.description")}
          descriptionMode={descriptionMode}
          grouped={grouped}
        >
          <Textarea
            value={prompt}
            onChange={(event) =>
              updateSetting("asr_initial_prompt", event.target.value)
            }
            disabled={isUpdating("asr_initial_prompt")}
            variant="compact"
            className="w-full min-h-[140px]"
            placeholder={t("settings.advanced.asrInitialPrompt.placeholder")}
          />
        </SettingContainer>
      </>
    );
  },
);
