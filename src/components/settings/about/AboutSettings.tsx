import React, { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";
import { Button } from "../../ui/Button";
import { AppDataDirectory } from "../AppDataDirectory";
import { AppLanguageSelector } from "../AppLanguageSelector";
import { ShowWhatsNewOnUpdate } from "../ShowWhatsNewOnUpdate";
import { ThemeSelector } from "../ThemeSelector";
import { LogDirectory } from "../debug";
import { commands } from "@/bindings";

export const AboutSettings: React.FC = () => {
  const { t } = useTranslation();
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

  const handleReportBugClick = async () => {
    try {
      let sysDetails = {
        os_version: "Unknown OS",
        cpu_model: "Unknown CPU",
        gpu_model: "Unknown GPU",
      };
      try {
        sysDetails = await commands.getSystemDetails();
      } catch (error) {
        console.error("Failed to get system details:", error);
      }

      const bodyTemplate = `## Before You Submit

**Please search [existing issues](https://github.com/cjpais/Handy/issues) to avoid duplicates.** Your bug may already be reported! Right now it's just me maintaining this project so many issues can be overwhelming! Help me out by checking first.

## Bug Description

A clear and concise description of what the bug is.

## System Information

**App Version:** ${version || "Unknown Version"}

<!-- You can find this in the app settings or about section -->

**Operating System:** ${sysDetails.os_version}

<!-- e.g., macOS 14.1, Windows 11, Ubuntu 22.04 -->

**CPU:** ${sysDetails.cpu_model}

<!-- e.g., Apple M2, Intel i7-12700K, AMD Ryzen 7 5800X -->

**GPU:** ${sysDetails.gpu_model}

<!-- e.g., Apple M2 GPU, NVIDIA RTX 4080, AMD RX 6800 XT, Intel UHD Graphics -->

## Logs

<!-- Please attach relevant logs to help us diagnose the issue. You can find the log directory by going to Settings > About in the app. -->`;

      const title = "[BUG - app]";
      const url = `https://github.com/cjpais/handy/issues/new?title=${encodeURIComponent(title)}&body=${encodeURIComponent(bodyTemplate)}`;
      await openUrl(url);
    } catch (error) {
      console.error("Failed to open bug report link:", error);
    }
  };

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("settings.about.title")}>
        <AppLanguageSelector descriptionMode="tooltip" grouped={true} />
        <ThemeSelector descriptionMode="tooltip" grouped={true} />
        <SettingContainer
          title={t("settings.about.version.title")}
          description={t("settings.about.version.description")}
          grouped={true}
        >
          {/* eslint-disable-next-line i18next/no-literal-string */}
          <span className="text-sm font-mono">v{version}</span>
        </SettingContainer>

        <ShowWhatsNewOnUpdate descriptionMode="tooltip" grouped={true} />

        <SettingContainer
          title={t("settings.about.supportDevelopment.title")}
          description={t("settings.about.supportDevelopment.description")}
          grouped={true}
        >
          <Button variant="primary" size="md" onClick={handleDonateClick}>
            {t("settings.about.supportDevelopment.button")}
          </Button>
        </SettingContainer>

        <SettingContainer
          title={t("settings.about.sourceCode.title")}
          description={t("settings.about.sourceCode.description")}
          grouped={true}
        >
          <Button
            variant="secondary"
            size="md"
            onClick={() => openUrl("https://github.com/cjpais/Handy")}
          >
            {t("settings.about.sourceCode.button")}
          </Button>
        </SettingContainer>

        <SettingContainer
          title={t("settings.about.reportBug.title")}
          description={t("settings.about.reportBug.description")}
          grouped={true}
        >
          <Button variant="secondary" size="md" onClick={handleReportBugClick}>
            {t("settings.about.reportBug.button")}
          </Button>
        </SettingContainer>

        <AppDataDirectory descriptionMode="tooltip" grouped={true} />
        <LogDirectory grouped={true} />
      </SettingsGroup>

      <SettingsGroup title={t("settings.about.acknowledgments.title")}>
        <SettingContainer
          title={t("settings.about.acknowledgments.ggml.title")}
          description={t("settings.about.acknowledgments.ggml.description")}
          grouped={true}
          layout="stacked"
        >
          <div className="text-sm text-mid-gray">
            {t("settings.about.acknowledgments.ggml.details")}
          </div>
        </SettingContainer>
      </SettingsGroup>
    </div>
  );
};
