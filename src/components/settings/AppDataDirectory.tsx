import React, { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { TextDisplay } from "../ui";

interface AppDataDirectoryProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const AppDataDirectory: React.FC<AppDataDirectoryProps> = ({
  descriptionMode = "inline",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const [appDirPath, setAppDirPath] = useState<string>("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const loadAppDirectory = async () => {
      try {
        const result = await invoke<string>("get_app_dir_path");
        setAppDirPath(result);
      } catch (err) {
        setError(
          err instanceof Error ? err.message : "Failed to load app directory",
        );
      } finally {
        setLoading(false);
      }
    };

    loadAppDirectory();
  }, []);

  const handleCopy = (value: string) => {
    // Could add a toast notification here if desired
    console.log("Copied to clipboard:", value);
  };

  if (loading) {
    return (
      <div className="animate-pulse">
        <div className="h-4 bg-gray-200 rounded w-1/3 mb-2"></div>
        <div className="h-8 bg-gray-100 rounded"></div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="p-4 bg-red-50 border border-red-200 rounded-lg">
        <p className="text-red-600 text-sm">
          {t("settings.about.app_data_directory.error", { error })}
        </p>
      </div>
    );
  }

  return (
    <TextDisplay
      label={t("settings.about.app_data_directory.label")}
      description={t("settings.about.app_data_directory.description")}
      value={appDirPath}
      descriptionMode={descriptionMode}
      grouped={grouped}
      copyable={true}
      monospace={true}
      onCopy={handleCopy}
    />
  );
};
