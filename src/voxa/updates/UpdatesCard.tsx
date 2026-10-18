import React from "react";
import { useTranslation } from "react-i18next";
import { Download, RefreshCw } from "lucide-react";
import { SettingContainer } from "@/components/ui/SettingContainer";
import { ToggleSwitch } from "@/components/ui/ToggleSwitch";
import { Button } from "@/components/ui/Button";
import { ProgressBar } from "@/components/shared";
import { useSettings } from "@/hooks/useSettings";
import { errorKey, useUpdateStore } from "./updateStore";

function formatDate(iso: string | null | undefined, language: string): string {
  if (!iso) return "";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "";
  return new Intl.DateTimeFormat(language, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}

/**
 * Rows of the "About" group that deal with updates: version status, the manual
 * check, the install button with release notes, and the automatic-check switch.
 */
export const UpdatesCard: React.FC = () => {
  const { t, i18n } = useTranslation();
  const { getSetting, updateSetting, isUpdating, updateChecksLocked } =
    useSettings();
  const { status, check, install } = useUpdateStore();

  const locked = updateChecksLocked !== false;
  const autoCheck = getSetting("update_checks_enabled") ?? true;
  const phase = status?.phase ?? "idle";
  const busy =
    phase === "checking" || phase === "downloading" || phase === "installing";

  const statusLine = (() => {
    switch (phase) {
      case "checking":
        return t("voxa.updates.checking");
      case "up_to_date":
        return t("voxa.updates.upToDate");
      case "available":
        return t("voxa.updates.available", { version: status?.version });
      case "downloading":
        return t("voxa.updates.downloading", {
          progress: status?.progress ?? 0,
        });
      case "installing":
        return t("voxa.updates.installing");
      case "error":
        return t(
          `voxa.updates.error.${errorKey(status?.error_kind ?? "other")}`,
        );
      default:
        return t("voxa.updates.idle");
    }
  })();

  const lastChecked = formatDate(status?.last_checked, i18n.language);
  const hasUpdate = status?.version != null && phase !== "up_to_date";
  const notes = hasUpdate ? status?.notes : null;

  const onToggle = async (enabled: boolean) => {
    await updateSetting("update_checks_enabled", enabled);
    if (enabled) void check();
  };

  return (
    <>
      <SettingContainer
        title={t("voxa.updates.title")}
        description={t("voxa.updates.description")}
        grouped={true}
        layout="stacked"
      >
        <div className="flex w-full items-start justify-between gap-4">
          <div className="min-w-0 space-y-1">
            <p
              className={`text-sm ${phase === "error" ? "text-red-400" : ""}`}
              role={phase === "error" ? "alert" : "status"}
              data-testid="update-status"
            >
              {statusLine}
            </p>
            {phase === "error" && status?.error_detail ? (
              <p className="text-xs text-mid-gray break-words">
                {status.error_detail}
              </p>
            ) : null}
            {lastChecked && phase !== "checking" ? (
              <p className="text-xs text-mid-gray">
                {t("voxa.updates.lastChecked", { time: lastChecked })}
              </p>
            ) : null}
          </div>
          <div className="flex shrink-0 gap-2">
            {phase === "available" || (phase === "error" && status?.version) ? (
              <Button
                variant="primary"
                size="md"
                disabled={busy || locked}
                onClick={() => void install()}
                data-testid="update-install"
              >
                <span className="inline-flex items-center gap-1.5">
                  <Download size={14} aria-hidden="true" />
                  {phase === "error"
                    ? t("voxa.updates.retry")
                    : t("voxa.updates.install")}
                </span>
              </Button>
            ) : null}
            <Button
              variant="secondary"
              size="md"
              disabled={busy || locked}
              onClick={() => void check()}
              data-testid="update-check"
            >
              <span className="inline-flex items-center gap-1.5">
                <RefreshCw
                  size={14}
                  aria-hidden="true"
                  className={phase === "checking" ? "animate-spin" : ""}
                />
                {t("voxa.updates.check")}
              </span>
            </Button>
          </div>
        </div>

        {phase === "downloading" ? (
          <div className="w-full pt-3">
            <ProgressBar
              progress={[{ id: "update", percentage: status?.progress ?? 0 }]}
              size="large"
            />
          </div>
        ) : null}

        {notes ? (
          <div className="w-full pt-3" data-testid="update-notes">
            <p className="text-xs font-medium uppercase tracking-wide text-mid-gray">
              {t("voxa.updates.notes", { version: status?.version })}
              {status?.published
                ? ` · ${formatDate(status.published, i18n.language)}`
                : ""}
            </p>
            <pre className="mt-1 max-h-48 overflow-auto whitespace-pre-wrap break-words font-sans text-sm text-text/80">
              {notes}
            </pre>
          </div>
        ) : null}
      </SettingContainer>

      <ToggleSwitch
        checked={locked ? false : autoCheck}
        onChange={(enabled) => void onToggle(enabled)}
        isUpdating={isUpdating("update_checks_enabled")}
        disabled={locked}
        label={t("voxa.updates.auto.title")}
        description={
          locked
            ? t("voxa.updates.auto.locked")
            : t("voxa.updates.auto.description")
        }
        descriptionMode="tooltip"
        grouped={true}
      />
    </>
  );
};
