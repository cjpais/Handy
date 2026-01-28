import React from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "../../hooks/useSettings";

import { ToggleSwitch } from "../ui/ToggleSwitch";
import { SettingContainer } from "../ui/SettingContainer";
import { Input } from "../ui/Input";

interface LocalApiServiceProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const LocalApiService: React.FC<LocalApiServiceProps> = React.memo(
  ({ grouped = false, descriptionMode = "inline" }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = (getSetting("local_api_enabled") as boolean) ?? false;
    const port = (getSetting("local_api_port") as number) ?? 5500;
    const [localPort, setLocalPort] = React.useState(port.toString());

    React.useEffect(() => {
      setLocalPort(port.toString());
    }, [port]);

    const handleEnabledChange = (checked: boolean) => {
      // This calls updateSetting in useSettingsStore, which we updated to handle 'local_api_enabled'
      // via commands.changeLocalApiSetting(checked)
      updateSetting("local_api_enabled", checked);
    };

    const handlePortChange = (e: React.ChangeEvent<HTMLInputElement>) => {
      const stringVal = e.target.value;
      setLocalPort(stringVal);
    };

    React.useEffect(() => {
      const timer = setTimeout(() => {
        const val = parseInt(localPort, 10);
        if (!isNaN(val) && val >= 1 && val <= 65535 && val !== port) {
          updateSetting("local_api_port", val);
        }
      }, 500);

      return () => clearTimeout(timer);
    }, [localPort, port, updateSetting]);

    return (
      <>
        <ToggleSwitch
          label={t("settings.advanced.local_api.label")}
          description={t("settings.advanced.local_api.description", { port })}
          checked={enabled}
          onChange={handleEnabledChange}
          isUpdating={isUpdating("local_api_enabled")}
          disabled={isUpdating("local_api_enabled")}
          grouped={grouped}
          descriptionMode={descriptionMode}
        />

        <SettingContainer
          title={t("settings.advanced.local_api.port.label")}
          description={t("settings.advanced.local_api.port.description")}
          grouped={grouped}
          descriptionMode={descriptionMode}
          disabled={!enabled}
        >
          <div className="flex items-center gap-2">
            <Input
              type="number"
              className="max-w-20"
              variant="compact"
              min={1}
              max={65535}
              value={localPort}
              onChange={handlePortChange}
              disabled={!enabled || isUpdating("local_api_port")}
            />
          </div>
        </SettingContainer>
      </>
    );
  },
);
