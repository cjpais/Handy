import React, { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { open, ask } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { commands } from "@/bindings";
import { useSettings } from "../../hooks/useSettings";
import { SettingContainer } from "../ui/SettingContainer";

interface ModelsDirectoryProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const ModelsDirectory: React.FC<ModelsDirectoryProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, refreshSettings } = useSettings();
    const [isBusy, setIsBusy] = useState(false);
    const [defaultPath, setDefaultPath] = useState<string>("");
    const [loading, setLoading] = useState(true);

    const rawCustomDir = getSetting("models_custom_dir");
    const customDir: string | null =
      typeof rawCustomDir === "string" ? rawCustomDir : null;
    const isCustom = customDir !== null;

    useEffect(() => {
      const loadDefaultPath = async () => {
        try {
          const result = await commands.getAppDirPath();
          if (result.status === "ok") {
            setDefaultPath(`${result.data}\\models`);
          }
        } catch (err) {
          console.error("Failed to load default models path:", err);
        } finally {
          setLoading(false);
        }
      };
      loadDefaultPath();
    }, []);

    const applyDirectoryChange = async (path: string | null) => {
      setIsBusy(true);
      try {
        const result = await commands.setModelsDirectory(path, true);
        if (result.status === "error") {
          toast.error(
            t("settings.debug.modelsDirectory.errorSetDir", {
              error: result.error,
            }),
          );
          return;
        }

        await refreshSettings();

        const { moved, skipped, failed } = result.data;
        if (moved > 0) {
          toast.success(
            t("settings.debug.modelsDirectory.moveResult_moved", {
              count: moved,
            }),
          );
        }
        if (skipped > 0) {
          toast.info(
            t("settings.debug.modelsDirectory.moveResult_skipped", {
              count: skipped,
            }),
          );
        }
        if (failed > 0) {
          toast.warning(
            t("settings.debug.modelsDirectory.moveResult_failed", {
              count: failed,
            }),
          );
        }
        if (moved === 0 && skipped === 0 && failed === 0) {
          toast.success(t("settings.debug.modelsDirectory.success"));
        }
      } catch (error) {
        toast.error(
          t("settings.debug.modelsDirectory.errorSetDir", {
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
        t("settings.debug.modelsDirectory.confirmDisableMessage"),
        {
          title: t("settings.debug.modelsDirectory.confirmDisableTitle"),
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
      } catch (openError) {
        console.error("Failed to open models folder:", openError);
      }
    };

    if (loading) {
      return (
        <SettingContainer
          title={t("settings.debug.modelsDirectory.title")}
          description={t("settings.debug.modelsDirectory.description")}
          descriptionMode={descriptionMode}
          grouped={grouped}
          layout="stacked"
        >
          <div className="animate-pulse">
            <div className="h-8 bg-gray-100 rounded" />
          </div>
        </SettingContainer>
      );
    }

    return (
      <SettingContainer
        title={t("settings.debug.modelsDirectory.title")}
        description={t("settings.debug.modelsDirectory.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
        layout="stacked"
      >
        <div className="flex items-center gap-2">
          <div className="flex-1 min-w-0 px-2 py-1.5 bg-mid-gray/10 border border-mid-gray/80 rounded-lg text-xs font-mono break-all select-text cursor-text">
            {isCustom ? customDir : defaultPath}
          </div>
          <button
            onClick={handleOpen}
            disabled={isBusy}
            className="p-1.5 rounded-lg border border-mid-gray/80 hover:bg-mid-gray/20 text-text/70 hover:text-text transition-colors disabled:opacity-50"
            title="Open this directory in your file manager"
          >
            <svg
              className="w-4 h-4"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M5 19a2 2 0 01-2-2V7a2 2 0 012-2h4l2 2h4a2 2 0 012 2v1M5 19h14a2 2 0 002-2v-5a2 2 0 00-2-2H9a2 2 0 00-2 2v5a2 2 0 01-2 2z"
              />
            </svg>
          </button>
          <button
            onClick={handleChange}
            disabled={isBusy}
            className="p-1.5 rounded-lg border border-mid-gray/80 hover:bg-mid-gray/20 text-text/70 hover:text-text transition-colors disabled:opacity-50"
            title="Select a custom directory for this type of data"
          >
            <svg
              className="w-4 h-4"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z"
              />
            </svg>
          </button>
          {isCustom && (
            <button
              onClick={handleRevert}
              disabled={isBusy}
              className="p-1.5 rounded-lg border border-mid-gray/80 hover:bg-mid-gray/20 text-text/70 hover:text-text transition-colors disabled:opacity-50"
              title="Reset this directory to its original default location"
            >
              <svg
                className="w-4 h-4"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M3 10h10a8 8 0 018 8v2M3 10l6 6m-6-6l6-6"
                />
              </svg>
            </button>
          )}
        </div>
      </SettingContainer>
    );
  },
);
