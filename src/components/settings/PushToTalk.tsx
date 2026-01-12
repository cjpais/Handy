import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";
import { commands } from "@/bindings";

interface PushToTalkProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const PushToTalk: React.FC<PushToTalkProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const [isWayland, setIsWayland] = useState(false);

    // Detect Wayland session
    useEffect(() => {
      const checkWayland = async () => {
        try {
          const wayland = await commands.isWaylandSession();
          setIsWayland(wayland);
        } catch (error) {
          console.error("Error checking Wayland session:", error);
        }
      };
      checkWayland();
    }, []);

    const pttEnabled = getSetting("push_to_talk") || false;

    // On Wayland, push-to-talk is not available (only toggle mode via SIGUSR2)
    if (isWayland) {
      return (
        <ToggleSwitch
          checked={false}
          onChange={() => {}}
          isUpdating={false}
          label={t("settings.general.pushToTalk.label")}
          description={t("settings.general.pushToTalk.waylandDisabled")}
          descriptionMode={descriptionMode}
          grouped={grouped}
          disabled={true}
        />
      );
    }

    return (
      <ToggleSwitch
        checked={pttEnabled}
        onChange={(enabled) => updateSetting("push_to_talk", enabled)}
        isUpdating={isUpdating("push_to_talk")}
        label={t("settings.general.pushToTalk.label")}
        description={t("settings.general.pushToTalk.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);
