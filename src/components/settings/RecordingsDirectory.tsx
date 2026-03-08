import React, { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { SettingContainer } from "../ui/SettingContainer";

interface RecordingsDirectoryProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const RecordingsDirectory: React.FC<RecordingsDirectoryProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const [defaultPath, setDefaultPath] = useState<string>("");
    const [loading, setLoading] = useState(true);

    useEffect(() => {
      const loadDefaultPath = async () => {
        try {
          const result = await commands.getAppDirPath();
          if (result.status === "ok") {
            setDefaultPath(`${result.data}\\recordings`);
          }
        } catch (err) {
          console.error("Failed to load default recordings path:", err);
        } finally {
          setLoading(false);
        }
      };
      loadDefaultPath();
    }, []);

    const handleOpen = async () => {
      try {
        await commands.openRecordingsFolder();
      } catch (openError) {
        console.error("Failed to open recordings folder:", openError);
      }
    };

    if (loading) {
      return (
        <SettingContainer
          title={t("settings.debug.recordingsDirectory.title")}
          description={t("settings.debug.recordingsDirectory.description")}
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
        title={t("settings.debug.recordingsDirectory.title")}
        description={t("settings.debug.recordingsDirectory.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
        layout="stacked"
      >
        <div className="flex items-center gap-2">
          <div className="flex-1 min-w-0 px-2 py-2 bg-mid-gray/10 border border-mid-gray/80 rounded-lg text-xs font-mono break-all select-text cursor-text">
            {defaultPath}
          </div>
          <button
            onClick={handleOpen}
            className="p-1.5 rounded-lg border border-mid-gray/80 hover:bg-mid-gray/20 text-text/70 hover:text-text transition-colors"
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
        </div>
      </SettingContainer>
    );
  });
