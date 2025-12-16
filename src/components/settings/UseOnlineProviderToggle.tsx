import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface UseOnlineProviderToggleProps {
    descriptionMode?: "inline" | "tooltip";
    grouped?: boolean;
}

export const UseOnlineProviderToggle: React.FC<UseOnlineProviderToggleProps> =
    React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
        const { t } = useTranslation();
        const { getSetting, updateSetting, isUpdating } = useSettings();

        const enabled = getSetting("use_online_provider") || false;

        return (
            <ToggleSwitch
                checked={enabled}
                onChange={(enabled) => updateSetting("use_online_provider", enabled)}
                isUpdating={isUpdating("use_online_provider")}
                label={t("settings.general.useOnlineProvider.label")}
                description={t("settings.general.useOnlineProvider.description")}
                descriptionMode={descriptionMode}
                grouped={grouped}
            />
        );
    });

UseOnlineProviderToggle.displayName = "UseOnlineProviderToggle";
