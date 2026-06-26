import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { RemoteDesktopAuthorizationCard } from "./RemoteDesktopAuthorizationCard";
import { RemoteDesktopAuthorizationWarning } from "./RemoteDesktopAuthorizationWarning";
import { RemoteDesktopTypingDelay } from "./RemoteDesktopTypingDelay";
import { useSettings } from "../../hooks/useSettings";
import { useOsType } from "../../hooks/useOsType";
import { useRemoteDesktopAuthorization } from "../../hooks/useRemoteDesktopAuthorization";
import { commands } from "@/bindings";
import type { PasteMethod, TypingTool } from "@/bindings";

interface TypingToolProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

const typingToolLabels: Record<string, string> = {
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
    const {
      isRelevant: isRemoteDesktopAuthorizationRelevant,
      isAuthorized: isRemoteDesktopAuthorized,
    } = useRemoteDesktopAuthorization(pasteMethod, selectedTool);
    const showAuthorizationWarning =
      isRemoteDesktopAuthorizationRelevant && !isRemoteDesktopAuthorized;

    // Only show this setting on Linux
    if (osType !== "linux") {
      return null;
    }

    const tools = availableTools ?? ["auto"];
    const typingToolOptions = tools.map((tool) =>
      tool === "auto"
        ? {
            value: "auto",
            label: t("settings.advanced.typingTool.options.auto"),
          }
        : { value: tool, label: typingToolLabels[tool] ?? tool },
    );
    const showTypingToolSetting = pasteMethod === "direct";

    const remoteDesktopAuthorizationControls = (
      <>
        <RemoteDesktopAuthorizationCard
          pasteMethod={pasteMethod}
          typingTool={selectedTool}
          descriptionMode={descriptionMode}
          grouped={grouped}
        />
        <RemoteDesktopTypingDelay
          pasteMethod={pasteMethod}
          typingTool={selectedTool}
          descriptionMode={descriptionMode}
          grouped={grouped}
        />
      </>
    );

    if (!showTypingToolSetting) {
      return remoteDesktopAuthorizationControls;
    }

    return (
      <div>
        <SettingContainer
          title={t("settings.advanced.typingTool.title")}
          description={t("settings.advanced.typingTool.description")}
          descriptionMode={descriptionMode}
          grouped={grouped}
          tooltipPosition="bottom"
        >
          <div className="flex items-center gap-2">
            {showAuthorizationWarning && <RemoteDesktopAuthorizationWarning />}
            <Dropdown
              options={typingToolOptions}
              selectedValue={selectedTool}
              onSelect={(value) =>
                updateSetting("typing_tool", value as TypingTool)
              }
              disabled={isUpdating("typing_tool")}
            />
          </div>
        </SettingContainer>
        {remoteDesktopAuthorizationControls}
      </div>
    );
  },
);
