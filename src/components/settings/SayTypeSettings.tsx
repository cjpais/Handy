import React, { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { SettingContainer } from "../ui/SettingContainer";
import { Input } from "../ui/Input";
import { Button } from "../ui/Button";

interface SayTypeConfig {
  enabled: boolean;
  port: number;
  token: string;
  onboarded: boolean;
}

interface SayTypeSettingsProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const SayTypeSettings: React.FC<SayTypeSettingsProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const [config, setConfig] = useState<SayTypeConfig | null>(null);
    const [localIp, setLocalIp] = useState<string>("");
    const [showToken, setShowToken] = useState(false);
    const [portInput, setPortInput] = useState("");
    const [isUpdating, setIsUpdating] = useState(false);

    const loadConfig = useCallback(async () => {
      try {
        const cfg = await invoke<SayTypeConfig>("saytype_get_config");
        setConfig(cfg);
        setPortInput(cfg.port.toString());
      } catch (e) {
        console.error("Failed to load SayType config:", e);
      }
    }, []);

    const loadLocalIp = useCallback(async () => {
      try {
        const ip = await invoke<string>("saytype_get_local_ip");
        setLocalIp(ip);
      } catch (e) {
        console.error("Failed to get local IP:", e);
      }
    }, []);

    useEffect(() => {
      loadConfig();
      loadLocalIp();
    }, [loadConfig, loadLocalIp]);

    const handleToggle = async (enabled: boolean) => {
      if (!config) return;
      setIsUpdating(true);
      try {
        const newConfig = { ...config, enabled };
        await invoke("saytype_set_config", { config: newConfig });
        setConfig(newConfig);
      } catch (e) {
        console.error("Failed to update SayType config:", e);
      } finally {
        setIsUpdating(false);
      }
    };

    const handleRegenerateToken = async () => {
      try {
        const newToken = await invoke<string>("saytype_regenerate_token");
        if (config) {
          setConfig({ ...config, token: newToken });
        }
      } catch (e) {
        console.error("Failed to regenerate token:", e);
      }
    };

    const handlePortChange = async () => {
      if (!config) return;
      const port = parseInt(portInput, 10);
      if (port >= 1024 && port <= 65535 && port !== config.port) {
        setIsUpdating(true);
        try {
          const newConfig = { ...config, port };
          await invoke("saytype_set_config", { config: newConfig });
          setConfig(newConfig);
        } catch (e) {
          console.error("Failed to update port:", e);
        } finally {
          setIsUpdating(false);
        }
      }
    };

    const copyToClipboard = async (text: string) => {
      await writeText(text);
    };

    if (!config) {
      return null;
    }

    const serverUrl = `http://${localIp}:${config.port}`;

    return (
      <div className="space-y-2">
        <ToggleSwitch
          checked={config.enabled}
          onChange={handleToggle}
          isUpdating={isUpdating}
          label={t("saytype.enableApi")}
          description={t("saytype.title")}
          descriptionMode={descriptionMode}
          grouped={grouped}
        />

        {config.enabled && (
          <div className="space-y-2">
            <SettingContainer
              title={t("saytype.serverAddress")}
              description={t("saytype.connectionInfo")}
              descriptionMode={descriptionMode}
              grouped={grouped}
            >
              <div className="flex items-center gap-2">
                <Input
                  type="text"
                  value={serverUrl}
                  readOnly
                  className="w-48"
                  variant="compact"
                />
                <Button
                  onClick={() => copyToClipboard(serverUrl)}
                  variant="secondary"
                  size="sm"
                >
                  {t("saytype.copy")}
                </Button>
              </div>
            </SettingContainer>

            <SettingContainer
              title={t("saytype.authToken")}
              description={t("saytype.connectionInfo")}
              descriptionMode={descriptionMode}
              grouped={grouped}
              layout="stacked"
            >
              <div className="flex flex-col gap-2">
                <div className="flex items-center gap-2">
                  <Input
                    type={showToken ? "text" : "password"}
                    value={config.token}
                    readOnly
                    className="flex-1 font-mono text-xs"
                    variant="compact"
                  />
                  <Button
                    onClick={() => setShowToken(!showToken)}
                    variant="secondary"
                    size="sm"
                  >
                    {showToken ? t("saytype.hide") : t("saytype.show")}
                  </Button>
                  <Button
                    onClick={() => copyToClipboard(config.token)}
                    variant="secondary"
                    size="sm"
                  >
                    {t("saytype.copy")}
                  </Button>
                </div>
                <Button
                  onClick={handleRegenerateToken}
                  variant="ghost"
                  size="sm"
                  className="self-start"
                >
                  {t("saytype.regenerateToken")}
                </Button>
              </div>
            </SettingContainer>

            <SettingContainer
              title={t("saytype.port")}
              description={t("saytype.portChangeNote")}
              descriptionMode="inline"
              grouped={grouped}
            >
              <Input
                type="number"
                value={portInput}
                onChange={(e) => setPortInput(e.target.value)}
                onBlur={handlePortChange}
                min={1024}
                max={65535}
                className="w-24"
                variant="compact"
                disabled={isUpdating}
              />
            </SettingContainer>
          </div>
        )}
      </div>
    );
  },
);
