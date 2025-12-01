import React, { useState, useEffect } from "react";
import { commands } from "@/bindings";
import { SettingContainer } from "../ui/SettingContainer";
import { Button } from "../ui/Button";

interface AppDataDirectoryProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const AppDataDirectory: React.FC<AppDataDirectoryProps> = ({
  descriptionMode = "inline",
  grouped = false,
}) => {
  const [appDirPath, setAppDirPath] = useState<string>("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const loadAppDirectory = async () => {
      try {
        const result = await commands.getAppDirPath();
        if (result.status === "ok") {
          setAppDirPath(result.data);
        } else {
          setError(result.error);
        }
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

  const handleOpen = async () => {
    if (!appDirPath) return;
    try {
      await commands.openAppDataDir();
    } catch (openError) {
      console.error("Failed to open app data directory:", openError);
    }
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
          Error loading app directory: {error}
        </p>
      </div>
    );
  }

  return (
    <SettingContainer
      title="App Data Directory"
      description="Main directory where application data, settings, and models are stored"
      descriptionMode={descriptionMode}
      grouped={grouped}
      layout="stacked"
    >
      <div className="flex items-center gap-2">
        <div className="flex-1 min-w-0 px-2 py-2 bg-mid-gray/10 border border-mid-gray/80 rounded text-xs font-mono break-all">
          {appDirPath}
        </div>
        <Button
          onClick={handleOpen}
          variant="secondary"
          size="sm"
          disabled={!appDirPath}
          className="px-3 py-2"
        >
          Open
        </Button>
      </div>
    </SettingContainer>
  );
};
