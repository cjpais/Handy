import React, { useState, useEffect } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";

import ModelSelector from "../model-selector";
import UpdateChecker from "../update-checker";

const Footer: React.FC = () => {
  const { t } = useTranslation();
  const [version, setVersion] = useState("");
  const [recordingMode, setRecordingMode] = useState<
    "transcribe" | "meeting" | "idle"
  >("idle");

  useEffect(() => {
    const fetchVersion = async () => {
      try {
        const appVersion = await getVersion();
        setVersion(appVersion);
      } catch (error) {
        console.error("Failed to get app version:", error);
        setVersion("0.1.2");
      }
    };

    fetchVersion();
  }, []);

  useEffect(() => {
    const unlisten = listen<{ mode: "transcribe" | "meeting" | "idle" }>(
      "recording-state-changed",
      (event) => {
        setRecordingMode(event.payload.mode);
      },
    );
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  return (
    <div className="w-full border-t border-mid-gray/20 pt-3">
      <div className="flex justify-between items-center text-xs px-4 pb-3 text-text/60">
        <div className="flex items-center gap-4">
          <ModelSelector />
          {recordingMode === "meeting" && (
            <div className="flex items-center gap-1.5 px-2 py-0.5 rounded-full bg-red-500/10 text-red-500 font-medium animate-pulse border border-red-500/20">
              <span className="w-2 h-2 rounded-full bg-red-500 animate-ping" />
              <span>{t("settings.meetings.activeIndicator")}</span>
            </div>
          )}
        </div>

        {/* Update Status */}
        <div className="flex items-center gap-1">
          <UpdateChecker />
          <span>•</span>
          {/* eslint-disable-next-line i18next/no-literal-string */}
          <span>v{version}</span>
        </div>
      </div>
    </div>
  );
};

export default Footer;
