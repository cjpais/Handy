import React from "react";
import { useTranslation } from "react-i18next";
import type { TranscriptionBackend } from "@/bindings";
import { useSettings } from "../../../hooks/useSettings";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { BackendSelector } from "./BackendSelector";
import { LiveSttPrivacyNotice } from "./LiveSttPrivacyNotice";
import { LiveSttSettingsSection } from "./LiveSttSettingsSection";

export const TranscriptionBackendSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, isUpdating, updateSetting } = useSettings();
  const backend = getSetting("transcription_backend") ?? "live_stt";

  const handleBackendChange = async (value: TranscriptionBackend) => {
    await updateSetting("transcription_backend", value);
  };

  return (
    <>
      <SettingsGroup title={t("settings.transcriptionBackend.title")}>
        <BackendSelector
          backend={backend}
          disabled={isUpdating("transcription_backend")}
          onChange={handleBackendChange}
        />
        <LiveSttPrivacyNotice backend={backend} />
      </SettingsGroup>

      {backend === "live_stt" && <LiveSttSettingsSection />}
    </>
  );
};
