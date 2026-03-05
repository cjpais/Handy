import React, { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";
import { Input } from "../../ui/Input";
import { useSettings } from "../../../hooks/useSettings";

export const NetworkSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting } = useSettings();

  const proxyUrl = getSetting("proxy_url") ?? "";
  const [localProxy, setLocalProxy] = useState(proxyUrl);

  useEffect(() => {
    setLocalProxy(proxyUrl ?? "");
  }, [proxyUrl]);

  const handleBlur = () => {
    const trimmed = localProxy.trim();
    updateSetting("proxy_url", trimmed || null);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") {
      (e.target as HTMLInputElement).blur();
    }
  };

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("settings.network.proxy.title")}>
        <SettingContainer
          title={t("settings.network.proxy.title")}
          description={t("settings.network.proxy.description")}
          descriptionMode="inline"
          grouped={true}
          layout="stacked"
        >
          <Input
            type="text"
            value={localProxy}
            onChange={(e) => setLocalProxy(e.target.value)}
            onBlur={handleBlur}
            onKeyDown={handleKeyDown}
            placeholder={t("settings.network.proxy.placeholder")}
            className="w-full font-mono"
            spellCheck={false}
            autoCorrect="off"
            autoCapitalize="off"
          />
        </SettingContainer>
      </SettingsGroup>
    </div>
  );
};
