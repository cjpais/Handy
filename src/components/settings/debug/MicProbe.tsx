import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { commands } from "@/bindings";
import type { MicProbeReport } from "@/bindings";
import { Button } from "../../ui/Button";
import { SettingContainer } from "../../ui/SettingContainer";

interface MicProbeProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/**
 * Debug tool for silent-microphone reports (#1213): opens a throwaway stream
 * on the configured mic and checks whether it actually delivers audio.
 * The full report (device list, configs, telemetry) is written to the log;
 * the UI shows the verdict.
 */
export const MicProbe: React.FC<MicProbeProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const [isRunning, setIsRunning] = useState(false);
  const [report, setReport] = useState<MicProbeReport | null>(null);

  const runProbe = async () => {
    setIsRunning(true);
    setReport(null);
    try {
      const result = await commands.probeMicrophone();
      if (result.status === "ok") {
        setReport(result.data);
      } else {
        toast.error(t("settings.debug.micProbe.error"));
      }
    } catch (error) {
      console.error("Microphone probe failed:", error);
      toast.error(t("settings.debug.micProbe.error"));
    } finally {
      setIsRunning(false);
    }
  };

  const verdictText = (r: MicProbeReport): string => {
    switch (r.verdict) {
      case "ok":
        return t("settings.debug.micProbe.verdict.ok", {
          ms: r.first_chunk_ms ?? 0,
        });
      case "silent":
        return t("settings.debug.micProbe.verdict.silent");
      case "open_failed":
        return t("settings.debug.micProbe.verdict.openFailed");
      case "device_not_found":
        return t("settings.debug.micProbe.verdict.deviceNotFound");
      case "busy":
        return t("settings.debug.micProbe.verdict.busy");
      default:
        return r.verdict;
    }
  };

  return (
    <SettingContainer
      title={t("settings.debug.micProbe.title")}
      description={t("settings.debug.micProbe.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <div className="flex items-center gap-2">
        {report && (
          <span
            className="text-xs max-w-64 truncate"
            title={report.error ?? report.device}
          >
            {verdictText(report)}
          </span>
        )}
        <Button
          variant="secondary"
          size="md"
          onClick={runProbe}
          disabled={isRunning}
        >
          {isRunning
            ? t("settings.debug.micProbe.running")
            : t("settings.debug.micProbe.button")}
        </Button>
      </div>
    </SettingContainer>
  );
};
