import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";
import { commands } from "@/bindings";

interface AutostartToggleProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const AutostartToggle: React.FC<AutostartToggleProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, refreshSettings } = useSettings();
    const [isUpdating, setIsUpdating] = useState(false);

    const autostartEnabled = getSetting("autostart_enabled") ?? false;

    const handleChange = async (enabled: boolean) => {
      setIsUpdating(true);
      try {
        const result = await commands.changeAutostartSetting(enabled);
        if (result.status === "error") {
          toast.error(t("settings.advanced.autostart.error"));
        }
        await refreshSettings();
      } finally {
        setIsUpdating(false);
      }
    };

    return (
      <ToggleSwitch
        checked={autostartEnabled}
        onChange={handleChange}
        isUpdating={isUpdating}
        label={t("settings.advanced.autostart.label")}
        description={t("settings.advanced.autostart.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);
