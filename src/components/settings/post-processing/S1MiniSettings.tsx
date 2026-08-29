import React, { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { ask } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  AlertCircle,
  CheckCircle2,
  Download,
  ExternalLink,
  Loader2,
  RotateCcw,
  Trash2,
  X,
} from "lucide-react";
import { useTranslation } from "react-i18next";

import {
  commands,
  type S1Context,
  type S1MiniStatus,
  type S1Structure,
  type S1Styling,
} from "@/bindings";
import { Alert } from "@/components/ui/Alert";
import { Button } from "@/components/ui/Button";
import { Dropdown } from "@/components/ui/Dropdown";
import { SettingContainer } from "@/components/ui/SettingContainer";
import { useSettings } from "@/hooks/useSettings";

const EMPTY_DOWNLOAD = {
  downloaded_bytes: 0,
  total_bytes: 0,
  progress: 0,
};
const S1_MINI_MODEL_URL = "https://huggingface.co/superwhisper/s1-mini";

const errorMessage = (error: unknown): string =>
  error instanceof Error ? error.message : String(error);
type S1MiniAction = "download" | "cancel" | "delete";

export const S1MiniSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, isUpdating, updateSetting } = useSettings();
  const [status, setStatus] = useState<S1MiniStatus | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [pendingAction, setPendingAction] = useState<S1MiniAction | null>(null);

  const refreshStatus = useCallback(async () => {
    try {
      setStatus(await commands.getS1MiniStatus());
    } catch (error) {
      setActionError(errorMessage(error));
    }
  }, []);

  useEffect(() => {
    let disposed = false;
    let disposeListener: (() => void) | undefined;

    void listen<S1MiniStatus>("s1-mini-status", (event) => {
      if (disposed) return;
      setStatus(event.payload);
      setActionError(null);
      if (event.payload.state === "downloading") {
        setPendingAction((current) =>
          current === "download" ? null : current,
        );
      }
    })
      .then((unlisten) => {
        if (disposed) {
          unlisten();
          return;
        }
        disposeListener = unlisten;
        void refreshStatus();
      })
      .catch((error) => {
        if (!disposed) {
          setActionError(errorMessage(error));
          void refreshStatus();
        }
      });

    return () => {
      disposed = true;
      disposeListener?.();
    };
  }, [refreshStatus]);

  const runAction = useCallback(
    async (
      action: () => Promise<
        { status: "ok" } | { status: "error"; error: string }
      >,
      actionName: S1MiniAction,
    ) => {
      setActionError(null);
      setPendingAction(actionName);
      try {
        const result = await action();
        if (result.status === "error") {
          setActionError(result.error);
        }
      } catch (error) {
        setActionError(errorMessage(error));
      } finally {
        setPendingAction((current) =>
          current === actionName ? null : current,
        );
        await refreshStatus();
      }
    },
    [refreshStatus],
  );

  const handleDownload = () => {
    setStatus((current) => ({
      ...EMPTY_DOWNLOAD,
      ...current,
      state: "downloading",
    }));
    void runAction(commands.downloadS1Mini, "download");
  };

  const handleCancel = () => {
    void runAction(commands.cancelS1MiniDownload, "cancel");
  };

  const handleDelete = async () => {
    const confirmed = await ask(
      t("settings.postProcessing.s1Mini.deleteConfirm"),
      {
        title: t("settings.postProcessing.s1Mini.deleteTitle"),
        kind: "warning",
      },
    );
    if (confirmed) {
      void runAction(commands.deleteS1Mini, "delete");
    }
  };

  const progress = Math.max(0, Math.min(100, status?.progress ?? 0));
  const statusLabel = status
    ? t(`settings.postProcessing.s1Mini.status.${status.state}`)
    : t("common.loading");
  const StatusIcon = useMemo(() => {
    switch (status?.state) {
      case "ready":
        return CheckCircle2;
      case "downloading":
        return Loader2;
      case "error":
        return AlertCircle;
      case "not_downloaded":
        return Download;
      default:
        return Loader2;
    }
  }, [status?.state]);

  const styling = getSetting("s1_styling") ?? "semi_formal";
  const structure = getSetting("s1_structure") ?? "prose";
  const context = getSetting("s1_context") ?? "general";

  const stylingOptions = useMemo(
    () =>
      (["casual", "semi_casual", "semi_formal", "formal"] as const).map(
        (value) => ({
          value,
          label: t(`settings.postProcessing.s1Mini.styling.options.${value}`),
        }),
      ),
    [t],
  );
  const structureOptions = useMemo(
    () =>
      (["prose", "lists"] as const).map((value) => ({
        value,
        label: t(`settings.postProcessing.s1Mini.structure.options.${value}`),
      })),
    [t],
  );
  const contextOptions = useMemo(
    () =>
      (["general", "email"] as const).map((value) => ({
        value,
        label: t(`settings.postProcessing.s1Mini.context.options.${value}`),
      })),
    [t],
  );

  return (
    <>
      <SettingContainer
        title={t("settings.postProcessing.api.model.title")}
        description={t("settings.postProcessing.s1Mini.description")}
        descriptionMode="inline"
        layout="stacked"
        grouped={true}
      >
        <div className="rounded-lg border border-mid-gray/20 bg-mid-gray/5 p-3 space-y-3">
          <div className="flex items-center justify-between gap-3">
            <button
              type="button"
              onClick={() => void openUrl(S1_MINI_MODEL_URL)}
              title={t("settings.postProcessing.s1Mini.modelDetails")}
              className="flex items-center gap-1.5 min-w-0 text-sm font-semibold hover:text-logo-primary transition-colors cursor-pointer"
            >
              <span className="truncate">
                {t("settings.postProcessing.s1Mini.brand")}
              </span>
              <ExternalLink className="h-3.5 w-3.5 shrink-0" />
            </button>
            <span className="text-xs text-mid-gray shrink-0">
              {t("settings.postProcessing.s1Mini.modelSize")}
            </span>
          </div>

          <div className="flex items-center justify-between gap-3">
            <div className="flex items-center gap-2 min-w-0">
              <StatusIcon
                className={`h-4 w-4 shrink-0 ${status?.state === "downloading" || !status ? "animate-spin" : ""}`}
              />
              <span className="text-sm font-medium truncate">
                {statusLabel}
              </span>
            </div>
            <div className="flex items-center gap-2 shrink-0">
              {status?.state === "not_downloaded" && (
                <Button
                  variant="primary-soft"
                  size="sm"
                  onClick={handleDownload}
                  disabled={pendingAction !== null}
                  className="flex items-center gap-1.5"
                >
                  <Download className="h-3.5 w-3.5" />
                  {t("settings.postProcessing.s1Mini.actions.download")}
                </Button>
              )}
              {status?.state === "downloading" && (
                <Button
                  variant="danger-ghost"
                  size="sm"
                  onClick={handleCancel}
                  disabled={pendingAction !== null}
                  className="flex items-center gap-1.5"
                >
                  <X className="h-3.5 w-3.5" />
                  {t("modelSelector.cancel")}
                </Button>
              )}
              {status?.state === "error" && (
                <Button
                  variant="primary-soft"
                  size="sm"
                  onClick={handleDownload}
                  disabled={pendingAction !== null}
                  className="flex items-center gap-1.5"
                >
                  <RotateCcw className="h-3.5 w-3.5" />
                  {t("settings.postProcessing.s1Mini.actions.retry")}
                </Button>
              )}
              {status &&
                status.state !== "downloading" &&
                status.downloaded_bytes > 0 && (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={handleDelete}
                    disabled={pendingAction !== null}
                    className="flex items-center gap-1.5"
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                    {t("common.delete")}
                  </Button>
                )}
              {!status && <Loader2 className="h-4 w-4 animate-spin" />}
            </div>
          </div>

          {status?.state === "downloading" && (
            <div className="space-y-1">
              <progress
                value={progress}
                max={100}
                aria-label={t("footer.downloading", {
                  progress: Math.round(progress),
                })}
                className="block w-full h-1.5 [&::-webkit-progress-bar]:rounded-full [&::-webkit-progress-bar]:bg-mid-gray/20 [&::-webkit-progress-value]:rounded-full [&::-webkit-progress-value]:bg-logo-primary"
              />
              <p className="text-xs text-mid-gray tabular-nums">
                {t("footer.downloading", {
                  progress: Math.round(progress),
                })}
              </p>
            </div>
          )}
          {(actionError || (status?.state === "error" && status.error)) && (
            <Alert variant="error" contained className="rounded-md">
              {actionError || (status?.state === "error" && status.error)}
            </Alert>
          )}
        </div>
      </SettingContainer>

      <SettingContainer
        title={t("settings.postProcessing.s1Mini.styling.title")}
        description={t("settings.postProcessing.s1Mini.styling.description")}
        descriptionMode="tooltip"
        grouped={true}
      >
        <Dropdown
          selectedValue={styling}
          options={stylingOptions}
          onSelect={(value) =>
            void updateSetting("s1_styling", value as S1Styling)
          }
          disabled={isUpdating("s1_styling")}
        />
      </SettingContainer>

      <SettingContainer
        title={t("settings.postProcessing.s1Mini.structure.title")}
        description={t("settings.postProcessing.s1Mini.structure.description")}
        descriptionMode="tooltip"
        grouped={true}
      >
        <Dropdown
          selectedValue={structure}
          options={structureOptions}
          onSelect={(value) =>
            void updateSetting("s1_structure", value as S1Structure)
          }
          disabled={isUpdating("s1_structure")}
        />
      </SettingContainer>

      <SettingContainer
        title={t("settings.postProcessing.s1Mini.context.title")}
        description={t("settings.postProcessing.s1Mini.context.description")}
        descriptionMode="tooltip"
        grouped={true}
      >
        <Dropdown
          selectedValue={context}
          options={contextOptions}
          onSelect={(value) =>
            void updateSetting("s1_context", value as S1Context)
          }
          disabled={isUpdating("s1_context")}
        />
      </SettingContainer>
    </>
  );
};
