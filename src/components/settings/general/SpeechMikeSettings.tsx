import React, { useEffect, useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";

interface SpeechMikeStatus {
  supported_platform: boolean;
  connected: boolean;
  blocked_by_other_app: boolean;
  device_name: string | null;
  vendor_id: number | null;
  product_id: number | null;
  serial_number: string | null;
  audio_device_name: string | null;
  buttons_enabled: boolean;
  auto_select_enabled: boolean;
  last_error: string | null;
  detected_blocking_processes: string[];
}

function toHex(n: number | null, digits: number): string {
  if (n === null) return "";
  return `0x${n.toString(16).toUpperCase().padStart(digits, "0")}`;
}

export const SpeechMikeSettings: React.FC = () => {
  const { t } = useTranslation();
  const [status, setStatus] = useState<SpeechMikeStatus | null>(null);

  const fetchStatus = useCallback(async () => {
    try {
      const s = await invoke<SpeechMikeStatus>("get_speechmike_status");
      setStatus(s);
    } catch (e) {
      console.error("get_speechmike_status failed:", e);
    }
  }, []);

  useEffect(() => {
    fetchStatus();

    const unlisten = Promise.all([
      listen<SpeechMikeStatus>("speechmike://connected", (e) =>
        setStatus(e.payload),
      ),
      listen("speechmike://disconnected", () => fetchStatus()),
      listen<SpeechMikeStatus>("speechmike://blocked-by-other-app", (e) =>
        setStatus(e.payload),
      ),
    ]);

    return () => {
      unlisten.then((fns) => fns.forEach((fn) => fn()));
    };
  }, [fetchStatus]);

  if (!status) return null;
  if (!status.supported_platform) return null;

  const handleAutoSelectToggle = async () => {
    const newValue = !status.auto_select_enabled;
    try {
      await invoke("set_speechmike_auto_select", { enabled: newValue });
      setStatus((prev) =>
        prev ? { ...prev, auto_select_enabled: newValue } : prev,
      );
    } catch (e) {
      console.error("set_speechmike_auto_select failed:", e);
    }
  };

  const handleButtonMappingToggle = async () => {
    const newValue = !status.buttons_enabled;
    try {
      await invoke("set_speechmike_button_mapping_enabled", {
        enabled: newValue,
      });
      setStatus((prev) =>
        prev ? { ...prev, buttons_enabled: newValue } : prev,
      );
    } catch (e) {
      console.error("set_speechmike_button_mapping_enabled failed:", e);
    }
  };

  const deviceLabel = status.connected
    ? `${status.device_name ?? t("settings.general.speechmike.unknownDevice")} • VID ${toHex(status.vendor_id, 4)} / PID ${toHex(status.product_id, 4)}`
    : t("settings.general.speechmike.notConnected");

  return (
    <SettingsGroup title={t("settings.general.speechmike.title")}>
      {/* Status row */}
      <SettingContainer
        title={t("settings.general.speechmike.status")}
        description={deviceLabel}
        descriptionMode="inline"
        grouped
      >
        <span
          className={`text-xs font-medium ${
            status.connected ? "text-green-600" : "text-slate-400"
          }`}
        >
          {status.connected
            ? t("settings.general.speechmike.connected")
            : t("settings.general.speechmike.disconnected")}
        </span>
      </SettingContainer>

      {/* Blocked-by-other-app warning */}
      {status.blocked_by_other_app && (
        <div className="px-4 py-3 bg-amber-50 border-t border-amber-200">
          <p className="text-sm font-medium text-amber-800">
            {t("settings.general.speechmike.blockedTitle")}
          </p>
          <p className="text-xs text-amber-700 mt-1">
            {status.detected_blocking_processes.length > 0
              ? t("settings.general.speechmike.blockedBy", {
                  processes: status.detected_blocking_processes.join(", "),
                })
              : t("settings.general.speechmike.blockedUnknown")}
          </p>
        </div>
      )}

      {/* Auto-select microphone toggle */}
      <SettingContainer
        title={t("settings.general.speechmike.autoSelect")}
        description={t("settings.general.speechmike.autoSelectDescription")}
        descriptionMode="tooltip"
        grouped
      >
        <button
          type="button"
          role="switch"
          aria-checked={status.auto_select_enabled}
          onClick={handleAutoSelectToggle}
          className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors focus:outline-none ${
            status.auto_select_enabled ? "bg-blue-600" : "bg-slate-200"
          }`}
        >
          <span
            className={`inline-block h-4 w-4 transform rounded-full bg-white shadow transition-transform ${
              status.auto_select_enabled ? "translate-x-6" : "translate-x-1"
            }`}
          />
        </button>
      </SettingContainer>

      {/* Button mapping toggle */}
      <SettingContainer
        title={t("settings.general.speechmike.buttonMapping")}
        description={t("settings.general.speechmike.buttonMappingDescription")}
        descriptionMode="tooltip"
        grouped
      >
        <button
          type="button"
          role="switch"
          aria-checked={status.buttons_enabled}
          onClick={handleButtonMappingToggle}
          className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors focus:outline-none ${
            status.buttons_enabled ? "bg-blue-600" : "bg-slate-200"
          }`}
        >
          <span
            className={`inline-block h-4 w-4 transform rounded-full bg-white shadow transition-transform ${
              status.buttons_enabled ? "translate-x-6" : "translate-x-1"
            }`}
          />
        </button>
      </SettingContainer>

      {/* Button mapping reference table */}
      {status.connected && (
        <div className="px-4 py-3 border-t border-slate-100">
          <p className="text-xs font-medium text-slate-500 mb-2">
            {t("settings.general.speechmike.mappingTable")}
          </p>
          <table className="text-xs w-full text-slate-600">
            <tbody className="divide-y divide-slate-100">
              {[
                ["REC", t("settings.general.speechmike.actionTranscribe")],
                ["STOP", t("settings.general.speechmike.actionCancel")],
                ["EOL", t("settings.general.speechmike.actionPostProcess")],
                [
                  "Trigger",
                  t("settings.general.speechmike.actionTranscribePtt"),
                ],
              ].map(([btn, action]) => (
                <tr key={btn}>
                  <td className="py-1 pr-4 font-mono font-medium">{btn}</td>
                  <td className="py-1 text-slate-500">{action}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* SpeechControl SDK link (Windows) */}
      <div className="px-4 py-2 border-t border-slate-100">
        <a
          href="https://www.dictation.philips.com/me/products/workflow-software/speechcontrol-device-and-application-control-software-lfh4000/"
          target="_blank"
          rel="noopener noreferrer"
          className="text-xs text-blue-500 hover:underline"
        >
          {t("settings.general.speechmike.learnSpeechControl")}
        </a>
      </div>
    </SettingsGroup>
  );
};
