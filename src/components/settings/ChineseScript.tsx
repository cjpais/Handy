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

    const scriptOptions = [
      {
        value: "as_transcribed",
        label: t("settings.advanced.chineseScript.options.asTranscribed"),
      },
      {
        value: "simplified",
        label: t("settings.advanced.chineseScript.options.simplified"),
      },
      {
        value: "traditional",
        label: t("settings.advanced.chineseScript.options.traditional"),
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
