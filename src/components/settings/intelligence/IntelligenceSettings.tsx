import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";
import { Dropdown, type DropdownOption } from "../../ui/Dropdown";
import { Button } from "../../ui/Button";
import { useSettings } from "../../../hooks/useSettings";
import { commands } from "@/bindings";

type ConnectionState =
  | { status: "idle" }
  | { status: "checking" }
  | { status: "ok"; models: string[] }
  | { status: "error"; message: string };

export const IntelligenceSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();

  const providerId = (getSetting("intelligence_provider_id") ??
    "ollama") as string;
  const model = (getSetting("intelligence_model") ?? "") as string;
  const providers = getSetting("post_process_providers") ?? [];

  const [connection, setConnection] = useState<ConnectionState>({
    status: "idle",
  });

  const providerOptions: DropdownOption[] = providers.map((p) => ({
    value: p.id,
    label: p.label,
  }));

  const testConnection = useCallback(async () => {
    setConnection({ status: "checking" });
    const result = await commands.testIntelligenceConnection();
    if (result.status === "ok") {
      setConnection({ status: "ok", models: result.data });
    } else {
      setConnection({ status: "error", message: result.error });
    }
  }, []);

  // Refresh the model list whenever the provider changes.
  useEffect(() => {
    testConnection();
  }, [providerId, testConnection]);

  const modelOptions: DropdownOption[] =
    connection.status === "ok"
      ? connection.models.map((m) => ({ value: m, label: m }))
      : model
        ? [{ value: model, label: model }]
        : [];

  return (
    <SettingsGroup title={t("settings.intelligence.title")}>
      <SettingContainer
        title={t("settings.intelligence.provider.title")}
        description={t("settings.intelligence.provider.description")}
        descriptionMode="tooltip"
        grouped
        layout="horizontal"
      >
        <Dropdown
          options={providerOptions}
          selectedValue={providerId}
          onSelect={(value) => updateSetting("intelligence_provider_id", value)}
          disabled={isUpdating("intelligence_provider_id")}
        />
      </SettingContainer>
      <SettingContainer
        title={t("settings.intelligence.model.title")}
        description={t("settings.intelligence.model.description")}
        descriptionMode="tooltip"
        grouped
        layout="horizontal"
      >
        <div className="flex items-center gap-2">
          <Dropdown
            options={modelOptions}
            selectedValue={model}
            onSelect={(value) => updateSetting("intelligence_model", value)}
            disabled={
              isUpdating("intelligence_model") || modelOptions.length === 0
            }
          />
          <Button
            variant="secondary"
            size="sm"
            onClick={testConnection}
            disabled={connection.status === "checking"}
          >
            {connection.status === "checking"
              ? t("settings.intelligence.status.checking")
              : t("settings.intelligence.testConnection")}
          </Button>
        </div>
      </SettingContainer>
      {connection.status === "error" && (
        <p className="text-xs text-red-400 px-4 pb-3">
          {t("settings.intelligence.status.error", {
            error: connection.message,
          })}
        </p>
      )}
      {connection.status === "ok" && connection.models.length === 0 && (
        <p className="text-xs text-muted-foreground px-4 pb-3">
          {t("settings.intelligence.status.noModels")}
        </p>
      )}
    </SettingsGroup>
  );
};
