import React, { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { open, ask } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { commands } from "@/bindings";
import { SettingContainer } from "../ui/SettingContainer";
import { PathDisplay } from "../ui/PathDisplay";

interface ModelsDirectoryProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const ModelsDirectory: React.FC<ModelsDirectoryProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const [isBusy, setIsBusy] = useState(false);
    const [modelsPath, setModelsPath] = useState<string>("");
    const [isCustom, setIsCustom] = useState(false);
    const [loading, setLoading] = useState(true);

    const refreshPath = useCallback(async () => {
      try {
        const pathResult = await commands.getModelsDirPath();
        if (pathResult.status === "ok") {
          setModelsPath(pathResult.data);
        }
        const settingsResult = await commands.getAppSettings();
        if (settingsResult.status === "ok") {
          setIsCustom(settingsResult.data.models_custom_dir != null);
        }
      } catch (err) {
        console.error("Failed to load models path:", err);
      } finally {
        setLoading(false);
      }
    }, []);

    useEffect(() => {
      refreshPath();
    }, [refreshPath]);

    const applyDirectoryChange = async (path: string | null) => {
      setIsBusy(true);
      try {
        const result = await commands.setModelsDirectory(path, true);
        if (result.status === "error") {
          toast.error(
            t("settings.about.modelsDirectory.errorSetDir", {
              error: result.error,
            }),
          );
          return;
        }

        const { moved, skipped, failed } = result.data;
        if (moved > 0) {
          toast.success(
            t("settings.about.modelsDirectory.moveResult_moved", {
              count: moved,
            }),
          );
        }
        if (skipped > 0) {
          toast.info(
            t("settings.about.modelsDirectory.moveResult_skipped", {
              count: skipped,
            }),
          );
        }
        if (failed > 0) {
          toast.warning(
            t("settings.about.modelsDirectory.moveResult_failed", {
              count: failed,
            }),
          );
        }
        if (moved === 0 && skipped === 0 && failed === 0) {
          toast.success(t("settings.about.modelsDirectory.success"));
        }

        await refreshPath();
      } catch (error) {
        toast.error(
          t("settings.about.modelsDirectory.errorSetDir", {
            error: String(error),
          }),
        );
      } finally {
        setIsBusy(false);
      }
    };

    const handleChange = async () => {
      const selected = await open({ directory: true, multiple: false });
      if (!selected) return;
      await applyDirectoryChange(selected as string);
    };

    const handleRevert = async () => {
      const confirmed = await ask(
        t("settings.about.modelsDirectory.confirmRevertMessage"),
        {
          title: t("settings.about.modelsDirectory.confirmRevertTitle"),
          okLabel: t("common.revert"),
          cancelLabel: t("common.cancel"),
          kind: "warning",
        },
      );
      if (!confirmed) return;
      await applyDirectoryChange(null);
    };

    const handleOpen = async () => {
      try {
        await commands.openModelsFolder();
      } catch (err) {
        console.error("Failed to open models folder:", err);
      }
    };

    return (
      <SettingContainer
        title={t("settings.about.modelsDirectory.title")}
        description={t("settings.about.modelsDirectory.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
        layout="stacked"
      >
        {loading ? (
          <div className="animate-pulse">
            <div className="h-8 bg-gray-100 rounded" />
          </div>
        ) : (
          <div className="flex items-center gap-2">
            <div className="flex-1 min-w-0">
              <PathDisplay
                path={modelsPath}
                onOpen={handleOpen}
                disabled={isBusy}
              />
            </div>
            <button
              onClick={handleChange}
              disabled={isBusy}
              className="px-2 py-1.5 text-xs rounded-lg border border-mid-gray/80 hover:bg-mid-gray/20 text-text/70 hover:text-text transition-colors disabled:opacity-50"
            >
              {t("common.change")}
            </button>
            {isCustom && (
              <button
                onClick={handleRevert}
                disabled={isBusy}
                className="px-2 py-1.5 text-xs rounded-lg border border-mid-gray/80 hover:bg-mid-gray/20 text-text/70 hover:text-text transition-colors disabled:opacity-50"
              >
                {t("common.revert")}
              </button>
            )}
          </div>
        )}
      </SettingContainer>
    );
  },
);
