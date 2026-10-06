import React, { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { commands, CliInstallStatus, Result } from "@/bindings";
import { SettingContainer } from "../ui/SettingContainer";
import { Button } from "../ui/Button";

interface CliInstallProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const CliInstall: React.FC<CliInstallProps> = ({
  descriptionMode = "inline",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const [status, setStatus] = useState<CliInstallStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadStatus = async () => {
    try {
      const result = await commands.getCliInstallStatus();
      if (result.status === "ok") {
        setStatus(result.data);
      } else {
        setError(result.error);
      }
    } catch (err) {
      setError(
        err instanceof Error
          ? err.message
          : "Failed to load CLI install status",
      );
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    loadStatus();
  }, []);

  const handleAction = async (
    action: () => Promise<Result<CliInstallStatus, string>>,
  ) => {
    setBusy(true);
    setError(null);
    try {
      const result = await action();
      if (result.status === "ok") {
        setStatus(result.data);
      } else {
        setError(result.error);
      }
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "CLI install action failed",
      );
    } finally {
      setBusy(false);
    }
  };

  const handleInstall = () => {
    void handleAction(() => commands.installCli());
  };

  const handleUninstall = () => {
    void handleAction(() => commands.uninstallCli());
  };

  if (loading) {
    return (
      <div className="animate-pulse">
        <div className="h-4 bg-gray-200 rounded w-1/3 mb-2"></div>
        <div className="h-8 bg-gray-100 rounded"></div>
      </div>
    );
  }

  if (error && !status) {
    return (
      <div className="p-4 bg-red-50 border border-red-200 rounded-lg">
        <p className="text-red-600 text-sm">
          {t("errors.cliInstallStatus", { error })}
        </p>
      </div>
    );
  }

  return (
    <SettingContainer
      title={t("settings.about.cliInstall.title")}
      description={t("settings.about.cliInstall.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
      layout="stacked"
    >
      <div className="flex flex-col gap-2">
        <div className="flex items-center gap-2">
          <Button
            onClick={status?.installed ? handleUninstall : handleInstall}
            disabled={busy}
            variant={status?.installed ? "danger" : "primary"}
          >
            {status?.installed
              ? t("settings.about.cliInstall.uninstall")
              : t("settings.about.cliInstall.install")}
          </Button>
          <span className="text-sm text-gray-600">
            {status?.installed
              ? t("settings.about.cliInstall.installedAt", {
                  path: status.link_path,
                })
              : t("settings.about.cliInstall.notInstalled")}
          </span>
        </div>
        {status?.installed && !status.dir_on_path && (
          <p className="text-sm text-amber-600">
            {t("settings.about.cliInstall.notOnPath", {
              path: status.link_path,
            })}
          </p>
        )}
        {error && (
          <div className="p-3 bg-red-50 border border-red-200 rounded-lg">
            <p className="text-red-600 text-sm">
              {t("errors.cliInstallAction", { error })}
            </p>
          </div>
        )}
      </div>
    </SettingContainer>
  );
};
