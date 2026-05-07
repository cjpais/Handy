import React, { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { useSettings } from "../../../hooks/useSettings";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { LiveSttAdvancedSettings } from "./LiveSttAdvancedSettings";
import { LiveSttAuthSettings } from "./LiveSttAuthSettings";
import { LiveSttServerSettings } from "./LiveSttServerSettings";
import {
  normalizeLiveSttServerUrlInput,
  validateLiveSttServerUrlInput,
} from "./livesttValidation";

export const LiveSttSettingsSection: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, isUpdating, refreshSettings } = useSettings();
  const serverUrl = getSetting("livestt_server_url") ?? "";
  const consultationId = getSetting("livestt_consultation_id") ?? "";
  const finalizeTimeoutMs = getSetting("livestt_finalize_timeout_ms") ?? 15000;

  const [serverUrlInput, setServerUrlInput] = useState(serverUrl);
  const [serverUrlError, setServerUrlError] = useState<string | null>(null);
  const [authRefreshKey, setAuthRefreshKey] = useState(0);

  useEffect(() => {
    setServerUrlInput(serverUrl);
  }, [serverUrl]);

  const serverUrlValidation = useMemo(
    () => normalizeLiveSttServerUrlInput(serverUrlInput),
    [serverUrlInput],
  );

  const saveServerUrlIfValid = async (): Promise<string | null> => {
    const errorKey = validateLiveSttServerUrlInput(serverUrlInput);
    if (errorKey) {
      setServerUrlError(t(errorKey));
      return null;
    }

    setServerUrlError(null);
    if (serverUrlValidation.normalized !== serverUrl) {
      const result = await commands.changeLivesttServerUrlSetting(
        serverUrlValidation.normalized,
      );

      if (result.status === "error") {
        setServerUrlError(result.error);
        return null;
      }

      await refreshSettings();
      setAuthRefreshKey((value) => value + 1);
    }

    return serverUrlValidation.normalized;
  };

  const handleServerUrlChange = (value: string) => {
    setServerUrlInput(value);
    if (serverUrlError) {
      setServerUrlError(null);
    }
  };

  const handleServerUrlBlur = async () => {
    await saveServerUrlIfValid();
  };

  return (
    <SettingsGroup title={t("settings.transcriptionBackend.livestt.title")}>
      <LiveSttServerSettings
        value={serverUrlInput}
        error={serverUrlError}
        disabled={isUpdating("livestt_server_url")}
        onChange={handleServerUrlChange}
        onBlur={handleServerUrlBlur}
      />
      <LiveSttAuthSettings
        serverUrlInput={serverUrlInput}
        serverUrlValidation={serverUrlValidation}
        saveServerUrlIfValid={saveServerUrlIfValid}
        refreshKey={authRefreshKey}
      />
      <LiveSttAdvancedSettings
        consultationId={consultationId}
        finalizeTimeoutMs={finalizeTimeoutMs}
      />
    </SettingsGroup>
  );
};
