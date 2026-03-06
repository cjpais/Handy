import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { save, open } from "@tauri-apps/plugin-dialog";
import { commands } from "@/bindings";
import { SettingContainer } from "../ui/SettingContainer";
import { Button } from "../ui/Button";
import { useSettings } from "@/hooks/useSettings";

interface ExportImportSettingsProps {
  grouped?: boolean;
}

export const ExportImportSettings: React.FC<ExportImportSettingsProps> = ({
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { refreshSettings } = useSettings();
  const [status, setStatus] = useState<string | null>(null);

  const handleExport = async () => {
    try {
      const path = await save({
        defaultPath: "parler-settings.json",
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!path) return;
      const result = await commands.exportSettings(path);
      if (result.status === "ok") {
        setStatus(t("settings.about.exportImport.exportSuccess"));
      } else {
        setStatus(result.error);
      }
    } catch (err) {
      setStatus(
        err instanceof Error ? err.message : "Export failed",
      );
    }
    setTimeout(() => setStatus(null), 3000);
  };

  const handleImport = async () => {
    try {
      const path = await open({
        multiple: false,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!path) return;
      const result = await commands.importSettings(path);
      if (result.status === "ok") {
        setStatus(t("settings.about.exportImport.importSuccess"));
        await refreshSettings();
      } else {
        setStatus(result.error);
      }
    } catch (err) {
      setStatus(
        err instanceof Error ? err.message : "Import failed",
      );
    }
    setTimeout(() => setStatus(null), 3000);
  };

  return (
    <SettingContainer
      title={t("settings.about.exportImport.title")}
      description={t("settings.about.exportImport.description")}
      grouped={grouped}
      layout="stacked"
    >
      <div className="flex items-center gap-2">
        <Button variant="secondary" size="md" onClick={handleExport}>
          {t("settings.about.exportImport.export")}
        </Button>
        <Button variant="secondary" size="md" onClick={handleImport}>
          {t("settings.about.exportImport.import")}
        </Button>
        {status && (
          <span className="text-xs text-mid-gray">{status}</span>
        )}
      </div>
    </SettingContainer>
  );
};
