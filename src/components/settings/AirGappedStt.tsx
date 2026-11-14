import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { Input } from "../ui/Input";
import { useSettings } from "../../hooks/useSettings";
import type { SttHttpSettings } from "../../bindings";

/** One finished attempt to reach the backend, pushed from the Rust side. */
type SttHttpStatus = {
  ok: boolean;
  endpoint: string;
  language?: string | null;
  at: number;
};

/** `http://host:8080/v1` -> `host:8080`; keeps unparsable values as-is. */
const endpointHost = (endpoint: string) => {
  try {
    return new URL(endpoint).host || endpoint;
  } catch {
    return endpoint;
  }
};

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
  const [status, setStatus] = useState<SttHttpStatus | null>(null);

  // The backend reports every attempt, so the panel can show that a recording
  // really left for the endpoint — without this the switch is a silent promise.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    listen<SttHttpStatus>("stt-http-status", (event) => {
      setStatus(event.payload);
    }).then((off) => {
      if (disposed) {
        off();
      } else {
        unlisten = off;
      }
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

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
            {t("settings.advanced.airGappedStt.language")}
            <Input
              variant="compact"
              defaultValue={config.language ?? ""}
              placeholder="auto"
              onBlur={(event) =>
                patch({ language: event.target.value.trim() || null })
              }
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
          {status && (
            <div
              className={`flex items-center gap-2 text-xs ${
                status.ok ? "text-green-600" : "text-red-500"
              }`}
              title={`${status.endpoint}${
                status.language ? ` · ${status.language}` : ""
              }`}
            >
              <span aria-hidden="true">{status.ok ? "✓" : "✗"}</span>
              <span className="truncate">{endpointHost(status.endpoint)}</span>
              <span className="text-mid-gray">
                {new Date(status.at).toLocaleTimeString()}
              </span>
              {status.language && (
                <span className="text-mid-gray uppercase">{status.language}</span>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
};
