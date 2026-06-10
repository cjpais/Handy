import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ask, open } from "@tauri-apps/plugin-dialog";
import { commands } from "@/bindings";
import { useModelStore } from "@/stores/modelStore";
import { SettingContainer } from "../../ui/SettingContainer";
import { PathDisplay } from "../../ui/PathDisplay";
import { Button } from "../../ui/Button";

export const ModelsStorageDirectory: React.FC = () => {
  const { t } = useTranslation();
  const { loadModels } = useModelStore();
  const [modelsDirPath, setModelsDirPath] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refreshPath = useCallback(async () => {
    const result = await commands.getModelsDirPath();
    if (result.status === "ok") {
      setModelsDirPath(result.data);
      setError(null);
    } else {
      setError(result.error);
    }
  }, []);

  useEffect(() => {
    const load = async () => {
      try {
        await refreshPath();
      } catch (err) {
        setError(
          err instanceof Error ? err.message : "Failed to load models directory",
        );
      } finally {
        setLoading(false);
      }
    };

    void load();
  }, [refreshPath]);

  const applyStoragePath = async (
    path: string | null,
    migrate: boolean,
  ): Promise<void> => {
    setSaving(true);
    setError(null);

    try {
      const result = await commands.setModelsStorageDirectory(path, migrate);
      if (result.status === "error") {
        setError(result.error);
        return;
      }

      await refreshPath();
      await loadModels();
    } catch (err) {
      setError(
        err instanceof Error
          ? err.message
          : "Failed to update models storage directory",
      );
    } finally {
      setSaving(false);
    }
  };

  const handleOpen = async () => {
    if (!modelsDirPath) return;

    try {
      await commands.openModelsDir();
    } catch (openError) {
      console.error("Failed to open models directory:", openError);
    }
  };

  const handleBrowse = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: t("settings.models.storage.browseTitle"),
    });

    if (!selected || Array.isArray(selected)) {
      return;
    }

    const migrate = await ask(t("settings.models.storage.migratePrompt"), {
      title: t("settings.models.storage.migrateTitle"),
      kind: "info",
    });

    await applyStoragePath(selected, migrate);
  };

  const handleUseInstallDir = async () => {
    const installDirResult = await commands.getInstallModelsDirPath();
    if (installDirResult.status === "error") {
      setError(installDirResult.error);
      return;
    }

    const migrate = await ask(t("settings.models.storage.migratePrompt"), {
      title: t("settings.models.storage.migrateTitle"),
      kind: "info",
    });

    await applyStoragePath(installDirResult.data, migrate);
  };

  const handleResetToDefault = async () => {
    const migrate = await ask(t("settings.models.storage.migratePrompt"), {
      title: t("settings.models.storage.migrateTitle"),
      kind: "info",
    });

    await applyStoragePath(null, migrate);
  };

  if (loading) {
    return (
      <div className="animate-pulse">
        <div className="h-4 bg-gray-200 rounded w-1/3 mb-2" />
        <div className="h-8 bg-gray-100 rounded" />
      </div>
    );
  }

  return (
    <SettingContainer
      title={t("settings.models.storage.title")}
      description={t("settings.models.storage.description")}
      descriptionMode="inline"
      grouped
      layout="stacked"
    >
      <div className="space-y-3">
        {error && (
          <p className="text-sm text-red-500">{error}</p>
        )}
        <PathDisplay
          path={modelsDirPath}
          onOpen={handleOpen}
          disabled={!modelsDirPath || saving}
        />
        <div className="flex flex-wrap gap-2">
          <Button
            variant="secondary"
            size="sm"
            onClick={handleBrowse}
            disabled={saving}
          >
            {t("settings.models.storage.browse")}
          </Button>
          <Button
            variant="secondary"
            size="sm"
            onClick={handleUseInstallDir}
            disabled={saving}
          >
            {t("settings.models.storage.useInstallDir")}
          </Button>
          <Button
            variant="secondary"
            size="sm"
            onClick={handleResetToDefault}
            disabled={saving}
          >
            {t("settings.models.storage.resetDefault")}
          </Button>
        </div>
      </div>
    </SettingContainer>
  );
};
