import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { relaunch } from "@tauri-apps/plugin-process";
import { SettingContainer } from "../ui/SettingContainer";
import { Button } from "../ui/Button";

interface ResetAppDataProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const ResetAppData: React.FC<ResetAppDataProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const [showConfirm, setShowConfirm] = useState(false);
  const [isResetting, setIsResetting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleReset = async () => {
    setIsResetting(true);
    setError(null);

    try {
      const result = await commands.resetAppData();
      if (result.status === "ok") {
        // Relaunch the app to complete the reset
        await relaunch();
      } else {
        setError(result.error);
        setIsResetting(false);
        setShowConfirm(false);
      }
    } catch (err) {
      setError(String(err));
      setIsResetting(false);
      setShowConfirm(false);
    }
  };

  if (showConfirm) {
    return (
      <div
        className={`${grouped ? "px-4 p-2" : "px-4 p-2 rounded-lg border border-mid-gray/20"}`}
      >
        <div className="flex flex-col gap-3">
          <div className="flex items-center gap-2">
            <svg
              className="w-5 h-5 text-red-500 shrink-0"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"
              />
            </svg>
            <h3 className="text-sm font-medium text-red-500">
              {t("settings.advanced.resetApp.confirmTitle")}
            </h3>
          </div>
          <p className="text-sm text-text/70">
            {t("settings.advanced.resetApp.confirmDescription")}
          </p>
          {error && (
            <p className="text-sm text-red-400">{error}</p>
          )}
          <div className="flex gap-2 justify-end">
            <Button
              variant="secondary"
              size="sm"
              onClick={() => setShowConfirm(false)}
              disabled={isResetting}
            >
              {t("common.cancel")}
            </Button>
            <Button
              variant="danger"
              size="sm"
              onClick={handleReset}
              disabled={isResetting}
            >
              {isResetting
                ? t("settings.advanced.resetApp.resetting")
                : t("settings.advanced.resetApp.confirmButton")}
            </Button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <SettingContainer
      title={t("settings.advanced.resetApp.title")}
      description={t("settings.advanced.resetApp.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <Button variant="danger" size="md" onClick={() => setShowConfirm(true)}>
        {t("settings.advanced.resetApp.button")}
      </Button>
    </SettingContainer>
  );
};
