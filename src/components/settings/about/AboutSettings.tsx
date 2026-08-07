import React, { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";
import { Button } from "../../ui/Button";
import { AppDataDirectory } from "../AppDataDirectory";
import { AppLanguageSelector } from "../AppLanguageSelector";
import { ShowWhatsNewOnUpdate } from "../ShowWhatsNewOnUpdate";
import { ThemeSelector } from "../ThemeSelector";
import { LogDirectory } from "../debug";
import { commands } from "@/bindings";
import { Dialog } from "../../ui/Dialog";
import { Input } from "../../ui/Input";
import { Textarea } from "../../ui/Textarea";

export const AboutSettings: React.FC = () => {
  const { t } = useTranslation();
  const [version, setVersion] = useState("");
  const [isReportBugOpen, setIsReportBugOpen] = useState(false);
  const [bugTitle, setBugTitle] = useState("");
  const [bugDescription, setBugDescription] = useState("");
  const [includeLogs, setIncludeLogs] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);

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

  const handleReportBugClick = () => {
    setBugTitle("");
    setBugDescription("");
    setIncludeLogs(false);
    setSubmitError(null);
    setIsReportBugOpen(true);
  };

  const handleFormSubmit = async () => {
    setSubmitError(null);
    setIsSubmitting(true);
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

      let logsText = "";
      if (includeLogs) {
        try {
          const logsResult = await commands.readRecentLogs();
          if (logsResult.status === "error") {
            setSubmitError(t("settings.about.reportBug.logsFailed"));
            return;
          }
          logsText = logsResult.data;
          await writeText(logsText);
        } catch (error) {
          console.error("Failed to copy logs:", error);
          setSubmitError(t("settings.about.reportBug.logsFailed"));
          return;
        }
      }

      const bodyTemplate = `## ${t("settings.about.reportBug.issueTemplate.beforeSubmit")}

**${t("settings.about.reportBug.issueTemplate.searchExisting")}**

## ${t("settings.about.reportBug.issueTemplate.description")}

${bugDescription}

## ${t("settings.about.reportBug.issueTemplate.systemInformation")}

**${t("settings.about.reportBug.issueTemplate.appVersion")}:** ${version || t("settings.about.reportBug.issueTemplate.unknownVersion")}
**${t("settings.about.reportBug.issueTemplate.operatingSystem")}:** ${sysDetails.os_version}
**${t("settings.about.reportBug.issueTemplate.cpu")}:** ${sysDetails.cpu_model}
**${t("settings.about.reportBug.issueTemplate.gpu")}:** ${sysDetails.gpu_model}
${
  includeLogs
    ? `

> ${t("settings.about.reportBug.logsInstruction")}`
    : ""
}`;

      const title = `[${t("settings.about.reportBug.issueTemplate.titlePrefix")}] ${bugTitle}`;
      const url = `https://github.com/cjpais/handy/issues/new?title=${encodeURIComponent(title)}&body=${encodeURIComponent(bodyTemplate)}`;
      await openUrl(url);
      setIsReportBugOpen(false);
      setBugTitle("");
      setBugDescription("");
    } catch (error) {
      console.error("Failed to open bug report link:", error);
    } finally {
      setIsSubmitting(false);
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
          <Button variant="primary-soft" size="md" onClick={handleReportBugClick}>
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

      <Dialog
        open={isReportBugOpen}
        title={t("settings.about.reportBug.title")}
        closeLabel={t("common.cancel") || "Cancel"}
        onOpenChange={setIsReportBugOpen}
      >
        <div className="space-y-4 py-2 text-start">
          <div className="text-sm text-mid-gray bg-mid-gray/5 p-3 rounded-md border border-mid-gray/20">
            {t("settings.about.reportBug.searchPrompt")} {" "}
            <a
              href="https://github.com/cjpais/Handy/issues"
              target="_blank"
              rel="noopener noreferrer"
              className="text-logo-primary hover:underline font-semibold"
            >
              {t("settings.about.reportBug.existingIssuesLink")}
            </a>{" "}
            {t("settings.about.reportBug.searchPromptSuffix")}
          </div>

          <div className="flex flex-col space-y-1.5">
            <label className="text-xs font-semibold text-mid-gray uppercase tracking-wider">
              {t("settings.about.reportBug.titleLabel")}
            </label>
            <Input
              value={bugTitle}
              onChange={(e) => setBugTitle(e.target.value)}
              maxLength={120}
              placeholder={t("settings.about.reportBug.titlePlaceholder")}
              className="w-full font-medium"
              required
              disabled={isSubmitting}
            />
          </div>

          <div className="flex flex-col space-y-1.5">
            <label className="text-xs font-semibold text-mid-gray uppercase tracking-wider">
              {t("settings.about.reportBug.descriptionLabel")}
            </label>
            <Textarea
              value={bugDescription}
              onChange={(e) => setBugDescription(e.target.value)}
              maxLength={1500}
              placeholder={t("settings.about.reportBug.descriptionPlaceholder")}
              className="w-full min-h-[140px] font-medium"
              required
              disabled={isSubmitting}
            />
          </div>

          <label className="flex items-center space-x-2.5 text-sm cursor-pointer select-none py-1">
            <input
              type="checkbox"
              checked={includeLogs}
              onChange={(e) => setIncludeLogs(e.target.checked)}
              disabled={isSubmitting}
              className="w-4 h-4 rounded border-mid-gray/80 bg-mid-gray/10 text-logo-primary focus:ring-logo-primary accent-logo-primary"
            />
            <span className="font-semibold text-mid-gray">
              {t("settings.about.reportBug.includeLogs")}
            </span>
          </label>

          {includeLogs && (
            <div role="alert" className="rounded-md border border-amber-500/40 bg-amber-500/10 p-3 text-sm">
              <p className="font-semibold">{t("settings.about.reportBug.logsWarningTitle")}</p>
              <p className="mt-1 text-mid-gray">{t("settings.about.reportBug.logsWarning")}</p>
            </div>
          )}
          {submitError && <p role="alert" className="rounded-md border border-red-500/40 bg-red-500/10 p-3 text-sm text-red-400">{submitError}</p>}
          <div className="flex justify-end space-x-3 pt-3 border-t border-mid-gray/20">
            <Button
              variant="secondary"
              size="md"
              onClick={() => setIsReportBugOpen(false)}
              disabled={isSubmitting}
            >
              {t("common.cancel") || "Cancel"}
            </Button>
            <Button
              variant="primary"
              size="md"
              onClick={handleFormSubmit}
              disabled={!bugTitle.trim() || !bugDescription.trim() || isSubmitting}
            >
              {isSubmitting ? t("settings.about.reportBug.submitting") : t("settings.about.reportBug.submit")}
            </Button>
          </div>
        </div>
      </Dialog>
    </div>
  );
};
