import React from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import type { ChineseScript } from "@/bindings";

interface ChineseScriptProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const ChineseScriptSetting: React.FC<ChineseScriptProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    // The script's own name leads, so it reads the same in every language.
    const scriptOptions = [
      {
        value: "simplified",
        label: `简体 ${t("settings.advanced.chineseScript.options.simplified")}`,
      },
      {
        value: "traditional_tw",
        label: `繁體 ${t("settings.advanced.chineseScript.options.traditionalTw")}`,
      },
      {
        value: "traditional_hk",
        label: `繁體 ${t("settings.advanced.chineseScript.options.traditionalHk")}`,
      },
      {
        value: "as_transcribed",
        label: t("settings.advanced.chineseScript.options.asTranscribed"),
      },
    ];

    const selectedScript = (getSetting("chinese_script") ||
      "as_transcribed") as ChineseScript;

    return (
      <SettingContainer
        title={t("settings.advanced.chineseScript.title")}
        description={t("settings.advanced.chineseScript.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <Dropdown
          options={scriptOptions}
          selectedValue={selectedScript}
          onSelect={(value) =>
            updateSetting("chinese_script", value as ChineseScript)
          }
          disabled={isUpdating("chinese_script")}
        />
      </SettingContainer>
    );
  },
);
