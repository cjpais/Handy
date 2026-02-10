import React from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import { useOsType } from "../../hooks/useOsType";
import type { TypingTool } from "@/bindings";

interface TypingToolProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const TypingToolSetting: React.FC<TypingToolProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const osType = useOsType();

    // Only show this setting on Linux
    if (osType !== "linux") {
      return null;
    }

    const typingToolOptions = [
      {
        value: "auto",
        label: t("settings.advanced.typingTool.options.auto"),
      },
      {
        value: "wtype",
        label: "wtype",
      },
      {
        value: "kwtype",
        label: "kwtype",
      },
      {
        value: "dotool",
        label: "dotool",
      },
      {
        value: "ydotool",
        label: "ydotool",
      },
      {
        value: "xdotool",
        label: "xdotool",
      },
    ];

    const selectedTool = (getSetting("typing_tool") || "auto") as TypingTool;

    // Only show if paste method is "direct"
    const pasteMethod = getSetting("paste_method");
    if (pasteMethod !== "direct") {
      return null;
    }

    return (
      <SettingContainer
        title={t("settings.advanced.typingTool.title")}
        description={t("settings.advanced.typingTool.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
        tooltipPosition="bottom"
      >
        <Dropdown
          options={typingToolOptions}
          selectedValue={selectedTool}
          onSelect={(value) =>
            updateSetting("typing_tool", value as TypingTool)
          }
          disabled={isUpdating("typing_tool")}
        />
      </SettingContainer>
    );
  },
);
