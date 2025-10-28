import React, { useState, useEffect } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { SettingsGroup } from "../ui/SettingsGroup";
import { SettingContainer } from "../ui/SettingContainer";
import { Button } from "../ui/Button";

export const AboutSettings: React.FC = () => {
  const [version, setVersion] = useState("");

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

  const handleDonateClick = async () => {
    try {
      await openUrl("https://handy.computer/donate");
    } catch (error) {
      console.error("Failed to open donate link:", error);
    }
  };

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title="About">
        <SettingContainer
          title="Version"
          description="Current version of Handy"
          grouped={true}
        >
          <span className="text-sm font-mono">v{version}</span>
        </SettingContainer>

        <SettingContainer
          title="Source Code"
          description="View source code and contribute"
          grouped={true}
        >
          <Button
            variant="secondary"
            size="md"
            onClick={() => openUrl("https://github.com/cjpais/Handy")}
          >
            View on GitHub
          </Button>
        </SettingContainer>

        <SettingContainer
          title="Support Development"
          description="Help us continue building Handy"
          grouped={true}
        >
          <Button variant="primary" size="md" onClick={handleDonateClick}>
            Donate
          </Button>
        </SettingContainer>
      </SettingsGroup>

      <SettingsGroup title="Acknowledgments">
        <SettingContainer
          title="Whisper.cpp"
          description="High-performance inference of OpenAI's Whisper automatic speech recognition model"
          grouped={true}
          layout="stacked"
        >
          <div className="text-sm text-mid-gray">
            Handy uses Whisper.cpp for fast, local speech-to-text processing.
            Thanks to the amazing work by Georgi Gerganov and contributors.
          </div>
        </SettingContainer>
      </SettingsGroup>
    </div>
  );
};
