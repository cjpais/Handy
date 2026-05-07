import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "../../../hooks/useSettings";
import { Input } from "../../ui/Input";
import { SettingContainer } from "../../ui/SettingContainer";
import {
  MAX_FINALIZE_TIMEOUT_MS,
  MIN_FINALIZE_TIMEOUT_MS,
  parseConsultationIdInput,
  validateFinalizeTimeoutInput,
} from "./livesttValidation";

interface LiveSttAdvancedSettingsProps {
  consultationId: string;
  finalizeTimeoutMs: number;
}

export const LiveSttAdvancedSettings: React.FC<
  LiveSttAdvancedSettingsProps
> = ({ consultationId, finalizeTimeoutMs }) => {
  const { t } = useTranslation();
  const { isUpdating, updateSetting } = useSettings();
  const [consultationIdInput, setConsultationIdInput] =
    useState(consultationId);
  const [finalizeTimeoutInput, setFinalizeTimeoutInput] = useState(
    String(finalizeTimeoutMs),
  );
  const [consultationIdError, setConsultationIdError] = useState<string | null>(
    null,
  );
  const [finalizeTimeoutError, setFinalizeTimeoutError] = useState<
    string | null
  >(null);

  useEffect(() => {
    setConsultationIdInput(consultationId);
  }, [consultationId]);

  useEffect(() => {
    setFinalizeTimeoutInput(String(finalizeTimeoutMs));
  }, [finalizeTimeoutMs]);

  const handleConsultationIdBlur = async () => {
    const parsedValue = parseConsultationIdInput(consultationIdInput);
    if (parsedValue === null) {
      setConsultationIdError(
        t("settings.transcriptionBackend.livestt.consultationId.error"),
      );
      return;
    }

    setConsultationIdError(null);
    await updateSetting(
      "livestt_consultation_id",
      parsedValue === "" ? null : parsedValue,
    );
  };

  const handleFinalizeTimeoutBlur = async () => {
    const parsedValue = validateFinalizeTimeoutInput(finalizeTimeoutInput);
    if (parsedValue === null) {
      setFinalizeTimeoutError(
        t("settings.transcriptionBackend.livestt.finalizeTimeout.error"),
      );
      return;
    }

    setFinalizeTimeoutError(null);
    await updateSetting("livestt_finalize_timeout_ms", parsedValue);
  };

  return (
    <>
      <SettingContainer
        title={t("settings.transcriptionBackend.livestt.consultationId.title")}
        description={t(
          "settings.transcriptionBackend.livestt.consultationId.description",
        )}
        descriptionMode="tooltip"
        grouped={true}
        layout="stacked"
      >
        <Input
          type="number"
          min="1"
          step="1"
          value={consultationIdInput}
          onChange={(event) => {
            setConsultationIdInput(event.target.value);
            if (consultationIdError) {
              setConsultationIdError(null);
            }
          }}
          onBlur={() => {
            void handleConsultationIdBlur();
          }}
          placeholder={t(
            "settings.transcriptionBackend.livestt.consultationId.placeholder",
          )}
          disabled={isUpdating("livestt_consultation_id")}
          className="w-full"
        />
        {consultationIdError && (
          <p className="mt-2 text-xs text-red-500">{consultationIdError}</p>
        )}
      </SettingContainer>

      <SettingContainer
        title={t("settings.transcriptionBackend.livestt.finalizeTimeout.title")}
        description={t(
          "settings.transcriptionBackend.livestt.finalizeTimeout.description",
        )}
        descriptionMode="tooltip"
        grouped={true}
        layout="stacked"
      >
        <Input
          type="number"
          min={MIN_FINALIZE_TIMEOUT_MS}
          max={MAX_FINALIZE_TIMEOUT_MS}
          step="100"
          value={finalizeTimeoutInput}
          onChange={(event) => {
            setFinalizeTimeoutInput(event.target.value);
            if (finalizeTimeoutError) {
              setFinalizeTimeoutError(null);
            }
          }}
          onBlur={() => {
            void handleFinalizeTimeoutBlur();
          }}
          disabled={isUpdating("livestt_finalize_timeout_ms")}
          className="w-full"
        />
        {finalizeTimeoutError && (
          <p className="mt-2 text-xs text-red-500">{finalizeTimeoutError}</p>
        )}
      </SettingContainer>
    </>
  );
};
