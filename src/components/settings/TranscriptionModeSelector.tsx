import React from "react";
import { useTranslation } from "react-i18next";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import { commands } from "@/bindings";

interface TranscriptionModeSelectorProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

type TranscriptionMode = "local" | "cloud";

export const TranscriptionModeSelector: React.FC<TranscriptionModeSelectorProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, refreshSettings, isUpdating } = useSettings();
    const [isChanging, setIsChanging] = React.useState(false);

    const currentMode = (getSetting("transcription_mode") ||
      "local") as TranscriptionMode;

    const handleModeChange = async (mode: TranscriptionMode) => {
      if (mode === currentMode || isChanging) return;

      setIsChanging(true);
      try {
        const result = await commands.setTranscriptionMode(mode);
        if (result.status === "ok") {
          await refreshSettings();
        }
      } catch (error) {
        console.error("Failed to change transcription mode:", error);
      } finally {
        setIsChanging(false);
      }
    };

    const isDisabled = isUpdating("transcription_mode") || isChanging;

    return (
      <SettingContainer
        title={t("transcription.mode.label")}
        description={t("transcription.mode.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
        layout="stacked"
      >
        <div className="flex gap-2 w-full">
          <button
            type="button"
            onClick={() => handleModeChange("local")}
            disabled={isDisabled}
            className={`flex-1 px-4 py-3 rounded-lg border-2 transition-all duration-200 text-left ${
              currentMode === "local"
                ? "border-logo-primary bg-logo-primary/10"
                : "border-mid-gray/30 hover:border-mid-gray/50"
            } ${isDisabled ? "opacity-50 cursor-not-allowed" : "cursor-pointer"}`}
          >
            <div className="font-medium text-sm">
              {t("transcription.mode.local")}
            </div>
            <div className="text-xs text-mid-gray mt-1">
              {t("transcription.mode.localDescription")}
            </div>
          </button>
          <button
            type="button"
            onClick={() => handleModeChange("cloud")}
            disabled={isDisabled}
            className={`flex-1 px-4 py-3 rounded-lg border-2 transition-all duration-200 text-left ${
              currentMode === "cloud"
                ? "border-logo-primary bg-logo-primary/10"
                : "border-mid-gray/30 hover:border-mid-gray/50"
            } ${isDisabled ? "opacity-50 cursor-not-allowed" : "cursor-pointer"}`}
          >
            <div className="font-medium text-sm">
              {t("transcription.mode.cloud")}
            </div>
            <div className="text-xs text-mid-gray mt-1">
              {t("transcription.mode.cloudDescription")}
            </div>
          </button>
        </div>
      </SettingContainer>
    );
  });
