import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { Input } from "../ui/Input";
import { useSettings } from "../../hooks/useSettings";
import type { SttHttpSettings } from "../../bindings";

/**
 * Air-gapped speech-to-text.
 *
 * When enabled, recordings go to an OpenAI-compatible
 * `/audio/transcriptions` endpoint instead of the local model. Off by default,
 * so the app behaves exactly as before until someone turns it on.
 */
export const AirGappedStt: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();

  const config: SttHttpSettings = getSetting("stt_http") ?? {};
  const enabled = config.enabled ?? false;
  const patch = (next: Partial<SttHttpSettings>) =>
    updateSetting("stt_http", { ...config, ...next });

  return (
    <div className="flex flex-col">
      <ToggleSwitch
        checked={enabled}
        onChange={(value) => patch({ enabled: value })}
        isUpdating={isUpdating("stt_http")}
        label={t("settings.advanced.airGappedStt.label")}
        description={t("settings.advanced.airGappedStt.description")}
        descriptionMode="tooltip"
        grouped={true}
      />
      {enabled && (
        <div className="flex flex-col gap-2 px-3 pb-3 pt-1">
          <label className="flex flex-col gap-1 text-xs text-mid-gray">
            {t("settings.advanced.airGappedStt.baseUrl")}
            <Input
              variant="compact"
              defaultValue={config.base_url ?? ""}
              placeholder="http://127.0.0.1:8000/v1"
              onBlur={(event) => patch({ base_url: event.target.value.trim() })}
            />
          </label>
          <label className="flex flex-col gap-1 text-xs text-mid-gray">
            {t("settings.advanced.airGappedStt.model")}
            <Input
              variant="compact"
              defaultValue={config.model ?? ""}
              placeholder="whisper-1"
              onBlur={(event) => patch({ model: event.target.value.trim() })}
            />
          </label>
          <label className="flex flex-col gap-1 text-xs text-mid-gray">
            {t("settings.advanced.airGappedStt.apiKey")}
            <Input
              variant="compact"
              type="password"
              defaultValue={config.api_key ?? ""}
              placeholder={t("settings.advanced.airGappedStt.apiKeyPlaceholder")}
              onBlur={(event) => patch({ api_key: event.target.value.trim() })}
            />
          </label>
          <label className="flex flex-col gap-1 text-xs text-mid-gray">
            {t("settings.advanced.airGappedStt.timeout")}
            <Input
              variant="compact"
              type="number"
              min={5}
              max={600}
              defaultValue={config.timeout_secs ?? 120}
              onBlur={(event) =>
                patch({ timeout_secs: Number(event.target.value) || 120 })
              }
            />
          </label>
        </div>
      )}
    </div>
  );
};
