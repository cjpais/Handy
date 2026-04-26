import React, { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { commands } from "@/bindings";
import { useSettingsStore } from "@/stores/settingsStore";
import { useModelStore } from "@/stores/modelStore";
import { SettingContainer } from "../ui/SettingContainer";
import { PathDisplay } from "../ui/PathDisplay";
import { Button } from "../ui/Button";

interface ModelDirectoryProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const ModelDirectory: React.FC<ModelDirectoryProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const [modelDir, setModelDir] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const refreshSettings = useSettingsStore((state) => state.refreshSettings);
  const loadModels = useModelStore((state) => state.loadModels);
  const loadCurrentModel = useModelStore((state) => state.loadCurrentModel);

  const loadModelDirectory = async () => {
    try {
      const result = await commands.getModelDirPath();
      if (result.status === "ok") {
        setModelDir(result.data);
        setError(null);
      } else {
        setError(result.error);
      }
    } catch (err) {
      setError(
        err instanceof Error
          ? err.message
          : t("settings.about.modelDirectory.loadError"),
      );
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadModelDirectory();
  }, []);

  const handleOpen = async () => {
    if (!modelDir) return;

    try {
      await commands.openModelDir();
    } catch (openError) {
      console.error("Failed to open model directory:", openError);
    }
  };

  const applyModelDirectory = async (path: string | null) => {
    setSaving(true);
    try {
      const result = await commands.setModelStoragePath(path);
      if (result.status === "error") {
        throw new Error(result.error);
      }

      setModelDir(result.data);
      setError(null);
      await Promise.all([refreshSettings(), loadModels(), loadCurrentModel()]);
      toast.success(t("settings.about.modelDirectory.updated"));
    } catch (err) {
      const message =
        err instanceof Error
          ? err.message
          : t("settings.about.modelDirectory.updateError");
      setError(message);
      toast.error(message);
    } finally {
      setSaving(false);
    }
  };

  const handleChangeDirectory = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        defaultPath: modelDir || undefined,
        title: t("settings.about.modelDirectory.selectTitle"),
      });

      if (!selected || Array.isArray(selected) || selected === modelDir) {
        return;
      }

      await applyModelDirectory(selected);
    } catch (err) {
      const message =
        err instanceof Error
          ? err.message
          : t("settings.about.modelDirectory.updateError");
      setError(message);
      toast.error(message);
    }
  };

  const handleResetDirectory = async () => {
    await applyModelDirectory(null);
  };

  return (
    <SettingContainer
      title={t("settings.about.modelDirectory.title")}
      description={t("settings.about.modelDirectory.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
      layout="stacked"
    >
      {loading ? (
        <div className="animate-pulse">
          <div className="h-8 bg-gray-100 rounded" />
        </div>
      ) : (
        <div className="space-y-3">
          {error && (
            <div className="p-3 bg-red-50 border border-red-200 rounded text-xs text-red-600">
              {error}
            </div>
          )}
          <PathDisplay
            path={modelDir}
            onOpen={handleOpen}
            disabled={!modelDir}
          />
          <div className="flex flex-wrap gap-2">
            <Button
              variant="secondary"
              size="sm"
              onClick={() => void handleChangeDirectory()}
              disabled={saving}
            >
              {saving
                ? t("common.saving")
                : t("settings.about.modelDirectory.changeButton")}
            </Button>
            <Button
              variant="secondary"
              size="sm"
              onClick={() => void handleResetDirectory()}
              disabled={saving}
            >
              {t("settings.about.modelDirectory.resetButton")}
            </Button>
          </div>
          <p className="text-xs text-text/50">
            {t("settings.about.modelDirectory.hint")}
          </p>
          {!modelDir && (
            <div className="flex gap-2">
              <Button
                variant="secondary"
                size="sm"
                onClick={() => void loadModelDirectory()}
                disabled={saving}
              >
                {t("common.retry")}
              </Button>
            </div>
          )}
        </div>
      )}
    </SettingContainer>
  );
};
