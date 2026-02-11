import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { RemoteDesktopAuthorizationCard } from "./RemoteDesktopAuthorizationCard";
import { useSettings } from "../../hooks/useSettings";
import { useOsType } from "../../hooks/useOsType";
import { commands } from "@/bindings";
import type { PasteMethod, TypingTool } from "@/bindings";

interface TypingToolProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

const allToolLabels: Record<string, string> = {
  remote_desktop: "Remote Desktop (Portal)",
  wtype: "wtype",
  kwtype: "kwtype",
  dotool: "dotool",
  ydotool: "ydotool",
  xdotool: "xdotool",
};

export const TypingToolSetting: React.FC<TypingToolProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const osType = useOsType();
    const [availableTools, setAvailableTools] = useState<string[] | null>(null);

    useEffect(() => {
      if (osType !== "linux") return;
      commands
        .getAvailableTypingTools()
        .then(setAvailableTools)
        .catch(() => {
          setAvailableTools(["auto"]);
        });
    }, [osType]);

    const pasteMethod = (getSetting("paste_method") || "ctrl_v") as PasteMethod;
    const selectedTool = (getSetting("typing_tool") || "auto") as TypingTool;
    const isToolRelevant =
      selectedTool === "auto" || selectedTool === "remote_desktop";

    useEffect(() => {
      if (osType !== "linux") return;
      if (pasteMethod === "direct" && isToolRelevant) return;
      void commands.deleteRemoteDesktopAuthorization();
    }, [osType, pasteMethod, isToolRelevant]);

    // Only show this setting on Linux
    if (osType !== "linux") {
      return null;
    }

    // Only show if paste method is "direct"
    const currentPasteMethod = pasteMethod;
    if (currentPasteMethod !== "direct") {
      return null;
    }

    const tools = availableTools ?? ["auto"];
    const typingToolOptions = tools.map((tool) =>
      tool === "auto"
        ? {
            value: "auto",
            label: t("settings.advanced.typingTool.options.auto"),
          }
        : { value: tool, label: allToolLabels[tool] ?? tool },
    );

    return (
      <div>
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
        <RemoteDesktopAuthorizationCard typingTool={selectedTool} />
      </div>
    );
  },
);
