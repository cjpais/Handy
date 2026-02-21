import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { open, ask } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { commands } from "@/bindings";
import { useSettings } from "../../hooks/useSettings";
import { Button } from "../ui/Button";
import { SettingContainer } from "../ui/SettingContainer";

interface RecordingsDirectoryProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const RecordingsDirectory: React.FC<RecordingsDirectoryProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, refreshSettings } = useSettings();
    const [isBusy, setIsBusy] = useState(false);

    const rawCustomDir = getSetting("recordings_custom_dir");
    const customDir: string | null =
      typeof rawCustomDir === "string" ? rawCustomDir : null;
    const isCustomEnabled = customDir !== null;

    const applyDirectoryChange = async (path: string | null) => {
      setIsBusy(true);
      try {
        const result = await commands.setRecordingsDirectory(path, true);
        if (result.status === "error") {
          toast.error(
            t("settings.debug.recordingsDirectory.errorSetDir", {
              error: result.error,
            }),
          );
          return;
        }

        await refreshSettings();

        const { moved, skipped, failed } = result.data;
        if (moved > 0) {
          toast.success(
            t("settings.debug.recordingsDirectory.moveResult_moved", {
              count: moved,
            }),
          );
        }
        if (skipped > 0) {
          toast.info(
            t("settings.debug.recordingsDirectory.moveResult_skipped", {
              count: skipped,
            }),
          );
        }
        if (failed > 0) {
          toast.warning(
            t("settings.debug.recordingsDirectory.moveResult_failed", {
              count: failed,
            }),
          );
        }
      } catch (error) {
        toast.error(
          t("settings.debug.recordingsDirectory.errorSetDir", {
            error: String(error),
          }),
        );
      } finally {
        setIsBusy(false);
      }
    };

    const handleToggle = async (enabled: boolean) => {
      if (!enabled) {
        const confirmed = await ask(
          t("settings.debug.recordingsDirectory.confirmDisableMessage"),
          {
            title: t(
              "settings.debug.recordingsDirectory.confirmDisableTitle",
            ),
            okLabel: t(
              "settings.debug.recordingsDirectory.confirmDisableButton",
            ),
            cancelLabel: t("common.cancel"),
            kind: "warning",
          },
        );
        if (!confirmed) return;
        await applyDirectoryChange(null);
      } else {
        const selected = await open({ directory: true, multiple: false });
        if (!selected) return;
        await applyDirectoryChange(selected as string);
      }
    };

    const handleChooseFolder = async () => {
      const selected = await open({ directory: true, multiple: false });
      if (!selected) return;
      await applyDirectoryChange(selected as string);
    };

    return (
      <SettingContainer
        title={t("settings.debug.recordingsDirectory.title")}
        description={t("settings.debug.recordingsDirectory.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
        layout="stacked"
      >
        {/* Toggle row */}
        <div className="flex items-center justify-between mb-2">
          <span className="text-sm text-text/70">
            {isCustomEnabled
              ? customDir
              : t("settings.debug.recordingsDirectory.defaultPath")}
          </span>
          <label
            className={`inline-flex items-center ${isBusy ? "cursor-not-allowed" : "cursor-pointer"}`}
          >
            <input
              type="checkbox"
              className="sr-only peer"
              checked={isCustomEnabled}
              disabled={isBusy}
              onChange={(e) => handleToggle(e.target.checked)}
            />
            <div className="relative w-11 h-6 bg-mid-gray/20 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-logo-primary rounded-full peer peer-checked:after:translate-x-full rtl:peer-checked:after:-translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:start-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-background-ui peer-disabled:opacity-50"></div>
          </label>
        </div>

        {/* Folder picker row — only shown when custom folder is active */}
        {isCustomEnabled && (
          <Button
            variant="secondary"
            size="sm"
            disabled={isBusy}
            onClick={handleChooseFolder}
          >
            {t("settings.debug.recordingsDirectory.chooseFolder")}
          </Button>
        )}
      </SettingContainer>
    );
  });
