import React from "react";
import { useTranslation } from "react-i18next";
import { Input } from "../ui/Input";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import { useRemoteDesktopAuthorization } from "../../hooks/useRemoteDesktopAuthorization";
import type { PasteMethod, TypingTool } from "@/bindings";

interface RemoteDesktopTypingDelayProps {
  pasteMethod: PasteMethod;
  typingTool: TypingTool;
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/**
 * Controls the delay inserted after each Remote Desktop portal keyboard event.
 *
 * Inputs: current paste method, typing tool, and settings row display options.
 * Outputs: delay controls shown only when Remote Desktop typing authorization matters.
 * Side effects: persists the selected delay through Tauri settings commands.
 */
export const RemoteDesktopTypingDelay: React.FC<RemoteDesktopTypingDelayProps> =
  React.memo(
    ({
      pasteMethod,
      typingTool,
      descriptionMode = "tooltip",
      grouped = false,
    }) => {
      const { t } = useTranslation();
      const { settings, updateSetting, isUpdating } = useSettings();
      const { isRelevant } = useRemoteDesktopAuthorization(
        pasteMethod,
        typingTool,
      );

      if (!isRelevant) {
        return null;
      }

      const handleDelayChange = (
        event: React.ChangeEvent<HTMLInputElement>,
      ) => {
        const value = parseInt(event.target.value, 10);
        if (!isNaN(value) && value >= 0) {
          updateSetting("remote_desktop_key_event_delay_ms", value);
        }
      };

      return (
        <SettingContainer
          title={t("settings.advanced.typingDelay.title")}
          description={t("settings.advanced.typingDelay.description")}
          descriptionMode={descriptionMode}
          grouped={grouped}
          layout="horizontal"
          tooltipPosition="bottom"
        >
          <div className="flex items-center space-x-2">
            <Input
              type="number"
              min="0"
              max="100"
              value={settings?.remote_desktop_key_event_delay_ms ?? 5}
              onChange={handleDelayChange}
              disabled={isUpdating("remote_desktop_key_event_delay_ms")}
              className="w-20"
            />
            <span className="text-sm text-text">
              {t("settings.advanced.typingDelay.unit")}
            </span>
          </div>
        </SettingContainer>
      );
    },
  );
