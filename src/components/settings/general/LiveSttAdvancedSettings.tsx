import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { useSettings } from "../../../hooks/useSettings";
import { Input } from "../../ui/Input";
import { SettingContainer } from "../../ui/SettingContainer";
import { TagInput, type TagInputAddRejection } from "../../ui/TagInput";
import { Textarea } from "../../ui/Textarea";
import {
  MAX_FINALIZE_TIMEOUT_MS,
  MAX_LIVESTT_PROMPT_CHARS,
  MAX_LIVESTT_TERM_CHARS,
  MAX_LIVESTT_TERMS,
  MIN_FINALIZE_TIMEOUT_MS,
  normalizeLiveSttPromptInput,
  parseConsultationIdInput,
  validateFinalizeTimeoutInput,
} from "./livesttValidation";

interface LiveSttAdvancedSettingsProps {
  consultationId: string;
  finalizeTimeoutMs: number;
  prompt: string;
  terms: string[];
}

type FieldElement = HTMLInputElement | HTMLTextAreaElement;

function useSyncedTextField<TElement extends FieldElement>(
  externalValue: string,
) {
  const [value, setValue] = useState(externalValue);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setValue(externalValue);
  }, [externalValue]);

  const onChange = useCallback((event: React.ChangeEvent<TElement>) => {
    setValue(event.target.value);
    setError(null);
  }, []);

  return {
    value,
    setValue,
    error,
    setError,
    onChange,
  };
}

export const LiveSttAdvancedSettings: React.FC<
  LiveSttAdvancedSettingsProps
> = ({ consultationId, finalizeTimeoutMs, prompt, terms }) => {
  const { t } = useTranslation();
  const { isUpdating, updateSetting, refreshSettings } = useSettings();
  const [termsError, setTermsError] = useState<string | null>(null);

  const consultationIdField =
    useSyncedTextField<HTMLInputElement>(consultationId);

  const finalizeTimeoutField = useSyncedTextField<HTMLInputElement>(
    String(finalizeTimeoutMs),
  );

  const promptField = useSyncedTextField<HTMLTextAreaElement>(prompt);

  const handleConsultationIdBlur = useCallback(async () => {
    const parsedValue = parseConsultationIdInput(consultationIdField.value);

    if (parsedValue === null) {
      consultationIdField.setError(
        t("settings.transcriptionBackend.livestt.consultationId.error"),
      );
      return;
    }

    consultationIdField.setError(null);

    if (parsedValue !== consultationIdField.value) {
      consultationIdField.setValue(parsedValue);
    }

    if (parsedValue === consultationId) {
      return;
    }

    await updateSetting(
      "livestt_consultation_id",
      parsedValue === "" ? null : parsedValue,
    );
  }, [consultationId, consultationIdField, t, updateSetting]);

  const handlePromptBlur = useCallback(async () => {
    const { trimmed, isValid } = normalizeLiveSttPromptInput(promptField.value);

    if (!isValid) {
      promptField.setError(
        t("settings.transcriptionBackend.livestt.prompt.error"),
      );
      return;
    }

    promptField.setError(null);

    if (trimmed === prompt) {
      if (trimmed !== promptField.value) {
        promptField.setValue(trimmed);
      }

      return;
    }

    const result = await commands.changeLivesttPromptSetting(
      trimmed === "" ? null : trimmed,
    );

    if (result.status === "error") {
      promptField.setError(result.error);
      return;
    }

    promptField.setValue(trimmed);
    await refreshSettings();
  }, [prompt, promptField, refreshSettings, t]);

  const handleFinalizeTimeoutBlur = useCallback(async () => {
    const parsedValue = validateFinalizeTimeoutInput(
      finalizeTimeoutField.value,
    );

    if (parsedValue === null) {
      finalizeTimeoutField.setError(
        t("settings.transcriptionBackend.livestt.finalizeTimeout.error"),
      );
      return;
    }

    finalizeTimeoutField.setError(null);

    const normalizedValue = String(parsedValue);

    if (normalizedValue !== finalizeTimeoutField.value) {
      finalizeTimeoutField.setValue(normalizedValue);
    }

    if (parsedValue === finalizeTimeoutMs) {
      return;
    }

    await updateSetting("livestt_finalize_timeout_ms", parsedValue);
  }, [finalizeTimeoutField, finalizeTimeoutMs, t, updateSetting]);

  const promptCharCount = [...promptField.value].length;

  const handleTermsChange = useCallback(
    async (next: string[]) => {
      setTermsError(null);
      await updateSetting("livestt_terms", next);
    },
    [updateSetting],
  );

  const handleTermsAddRejected = useCallback(
    (reason: TagInputAddRejection) => {
      if (reason === "empty") {
        return;
      }
      setTermsError(
        t(`settings.transcriptionBackend.livestt.terms.errors.${reason}`),
      );
    },
    [t],
  );

  return (
    <>
      <SettingContainer
        title={t("settings.transcriptionBackend.livestt.prompt.title")}
        description={t(
          "settings.transcriptionBackend.livestt.prompt.description",
        )}
        descriptionMode="tooltip"
        grouped
        layout="stacked"
      >
        <Textarea
          value={promptField.value}
          onChange={promptField.onChange}
          onBlur={() => {
            void handlePromptBlur();
          }}
          placeholder={t(
            "settings.transcriptionBackend.livestt.prompt.placeholder",
          )}
          disabled={isUpdating("livestt_prompt")}
          maxLength={MAX_LIVESTT_PROMPT_CHARS}
          className="w-full"
        />

        <div className="mt-1 flex items-center justify-between text-xs text-slate-500">
          <span>
            {promptField.error && (
              <span className="text-red-500">{promptField.error}</span>
            )}
          </span>

          <span>
            {promptCharCount}/{MAX_LIVESTT_PROMPT_CHARS}
          </span>
        </div>
      </SettingContainer>

      <SettingContainer
        title={t("settings.transcriptionBackend.livestt.terms.title")}
        description={t(
          "settings.transcriptionBackend.livestt.terms.description",
        )}
        descriptionMode="tooltip"
        grouped
        layout="stacked"
      >
        <TagInput
          value={terms}
          onChange={(next) => {
            void handleTermsChange(next);
          }}
          onAddRejected={handleTermsAddRejected}
          placeholder={t(
            "settings.transcriptionBackend.livestt.terms.placeholder",
          )}
          disabled={isUpdating("livestt_terms")}
          maxTermLength={MAX_LIVESTT_TERM_CHARS}
          maxTerms={MAX_LIVESTT_TERMS}
          removeAriaLabel={t(
            "settings.transcriptionBackend.livestt.terms.remove",
          )}
        />

        <div className="mt-1 flex items-center justify-between text-xs text-slate-500">
          <span>
            {termsError && <span className="text-red-500">{termsError}</span>}
          </span>

          <span>
            {terms.length}/{MAX_LIVESTT_TERMS}
          </span>
        </div>
      </SettingContainer>

      <SettingContainer
        title={t("settings.transcriptionBackend.livestt.consultationId.title")}
        description={t(
          "settings.transcriptionBackend.livestt.consultationId.description",
        )}
        descriptionMode="tooltip"
        grouped
        layout="stacked"
      >
        <Input
          type="number"
          min="1"
          step="1"
          value={consultationIdField.value}
          onChange={consultationIdField.onChange}
          onBlur={() => {
            void handleConsultationIdBlur();
          }}
          placeholder={t(
            "settings.transcriptionBackend.livestt.consultationId.placeholder",
          )}
          disabled={isUpdating("livestt_consultation_id")}
          className="w-full"
        />

        {consultationIdField.error && (
          <p className="mt-2 text-xs text-red-500">
            {consultationIdField.error}
          </p>
        )}
      </SettingContainer>

      <SettingContainer
        title={t("settings.transcriptionBackend.livestt.finalizeTimeout.title")}
        description={t(
          "settings.transcriptionBackend.livestt.finalizeTimeout.description",
        )}
        descriptionMode="tooltip"
        grouped
        layout="stacked"
      >
        <Input
          type="number"
          min={MIN_FINALIZE_TIMEOUT_MS}
          max={MAX_FINALIZE_TIMEOUT_MS}
          step="100"
          value={finalizeTimeoutField.value}
          onChange={finalizeTimeoutField.onChange}
          onBlur={() => {
            void handleFinalizeTimeoutBlur();
          }}
          disabled={isUpdating("livestt_finalize_timeout_ms")}
          className="w-full"
        />

        {finalizeTimeoutField.error && (
          <p className="mt-2 text-xs text-red-500">
            {finalizeTimeoutField.error}
          </p>
        )}
      </SettingContainer>
    </>
  );
};
