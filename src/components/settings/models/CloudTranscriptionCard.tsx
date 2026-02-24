import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, ChevronUp, Cloud } from "lucide-react";
import { commands } from "@/bindings";
import Badge from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";

type TestStatus = "idle" | "testing" | "ok" | "error";

interface CloudTranscriptionCardProps {
  isActive: boolean;
  onSelect: (modelId: string) => void;
}

export const CloudTranscriptionCard: React.FC<CloudTranscriptionCardProps> = ({
  isActive,
  onSelect,
}) => {
  const { t } = useTranslation();
  const [isExpanded, setIsExpanded] = useState(false);
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [modelName, setModelName] = useState("");
  const [isSaving, setIsSaving] = useState(false);
  const [testStatus, setTestStatus] = useState<TestStatus>("idle");
  const [testError, setTestError] = useState<string | null>(null);
  const loadedRef = useRef(false);
  const okTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (loadedRef.current) return;
    loadedRef.current = true;
    commands.getAppSettings().then((result) => {
      if (result.status === "ok") {
        const s = result.data;
        setBaseUrl(s.cloud_transcription_base_url ?? "https://api.groq.com/openai/v1");
        setApiKey(s.cloud_transcription_api_key ?? "");
        setModelName(s.cloud_transcription_model ?? "whisper-large-v3");
      }
    });
  }, []);

  useEffect(() => {
    if (isActive) setIsExpanded(true);
  }, [isActive]);

  useEffect(() => () => { if (okTimerRef.current) clearTimeout(okTimerRef.current); }, []);

  const isConfigured =
    baseUrl.trim() !== "" && apiKey.trim() !== "" && modelName.trim() !== "";

  const saveField = async (
    field:
      | "cloud_transcription_base_url"
      | "cloud_transcription_api_key"
      | "cloud_transcription_model",
    value: string,
  ) => {
    setIsSaving(true);
    try {
      if (field === "cloud_transcription_base_url") {
        await commands.changeCloudTranscriptionBaseUrl(value);
      } else if (field === "cloud_transcription_api_key") {
        await commands.changeCloudTranscriptionApiKey(value);
      } else {
        await commands.changeCloudTranscriptionModel(value);
      }
    } catch (e) {
      console.error("Failed to save cloud setting:", e);
    } finally {
      setIsSaving(false);
    }
  };

  const handleTest = async () => {
    if (okTimerRef.current) clearTimeout(okTimerRef.current);
    setTestStatus("testing");
    setTestError(null);
    const result = await commands.testCloudTranscriptionConnection();
    if (result.status === "ok") {
      setTestStatus("ok");
      okTimerRef.current = setTimeout(() => setTestStatus("idle"), 2000);
    } else {
      setTestStatus("error");
      setTestError(result.error ?? t("settings.models.cloudTranscription.testFailed"));
    }
  };

  const containerClasses = [
    "flex flex-col rounded-xl px-4 py-3 gap-2 border-2 transition-all duration-200",
    isActive
      ? "border-logo-primary/50 bg-logo-primary/10"
      : "border-mid-gray/20 hover:border-logo-primary/30",
  ].join(" ");

  const testLabel =
    testStatus === "testing"
      ? t("settings.models.cloudTranscription.testing")
      : testStatus === "ok"
        ? t("settings.models.cloudTranscription.testOk")
        : t("settings.models.cloudTranscription.test");

  return (
    <div className={containerClasses}>
      {/* Header */}
      <button
        type="button"
        className="flex items-start justify-between w-full text-left"
        onClick={() => setIsExpanded((v) => !v)}
      >
        <div className="flex flex-col items-start flex-1 min-w-0">
          <div className="flex items-center gap-3 flex-wrap">
            <div className="flex items-center gap-1.5">
              <Cloud className="w-4 h-4 text-text/60 shrink-0" />
              <h3 className="text-base font-semibold text-text">
                {t("settings.models.cloudTranscription.title")}
              </h3>
            </div>
            {isActive ? (
              <Badge variant="primary">
                {t("modelSelector.active")}
              </Badge>
            ) : isConfigured ? (
              <Badge variant="secondary">
                {t("settings.models.cloudTranscription.configured")}
              </Badge>
            ) : (
              <Badge variant="secondary">
                {t("settings.models.cloudTranscription.notConfigured")}
              </Badge>
            )}
          </div>
          <p className="text-sm text-text/60 leading-relaxed">
            {t("settings.models.cloudTranscription.description")}
          </p>
        </div>
        <div className="ml-3 mt-0.5 shrink-0">
          {isExpanded ? (
            <ChevronUp className="w-4 h-4 text-text/40" />
          ) : (
            <ChevronDown className="w-4 h-4 text-text/40" />
          )}
        </div>
      </button>

      {/* Expanded config */}
      {isExpanded && (
        <>
          <hr className="w-full border-mid-gray/20" />
          <div className="flex flex-col gap-3">
            <div className="flex flex-col gap-1">
              <label className="text-xs font-medium text-text/60">
                {t("settings.models.cloudTranscription.baseUrlLabel")}
              </label>
              <Input
                type="text"
                variant="compact"
                value={baseUrl}
                onChange={(e) => setBaseUrl(e.target.value)}
                onBlur={(e) => saveField("cloud_transcription_base_url", e.target.value)}
                placeholder={t("settings.models.cloudTranscription.baseUrlPlaceholder")}
                className="w-full"
                disabled={isSaving}
              />
            </div>

            <div className="flex flex-col gap-1">
              <label className="text-xs font-medium text-text/60">
                {t("settings.models.cloudTranscription.apiKeyLabel")}
              </label>
              <Input
                type="password"
                variant="compact"
                value={apiKey}
                onChange={(e) => { setApiKey(e.target.value); setTestStatus("idle"); }}
                onBlur={(e) => saveField("cloud_transcription_api_key", e.target.value)}
                placeholder={t("settings.models.cloudTranscription.apiKeyPlaceholder")}
                className="w-full"
                disabled={isSaving}
              />
              {testStatus === "error" && (
                <p className="text-xs text-red-400 break-all">{testError}</p>
              )}
            </div>

            <div className="flex flex-col gap-1">
              <label className="text-xs font-medium text-text/60">
                {t("settings.models.cloudTranscription.modelLabel")}
              </label>
              <Input
                type="text"
                variant="compact"
                value={modelName}
                onChange={(e) => setModelName(e.target.value)}
                onBlur={(e) => saveField("cloud_transcription_model", e.target.value)}
                placeholder={t("settings.models.cloudTranscription.modelPlaceholder")}
                className="w-full"
                disabled={isSaving}
              />
            </div>

            {/* Bottom row: Test + Toggle */}
            <div className="flex items-center justify-end gap-3">
              <Button
                variant="secondary"
                size="sm"
                onClick={() => void handleTest()}
                disabled={!apiKey.trim() || testStatus === "testing"}
                className={[
                  "w-16 justify-center shrink-0",
                  testStatus === "ok" ? "!text-green-500" : "",
                ].join(" ")}
              >
                {testLabel}
              </Button>

              {/* Toggle switch */}
              <label
                className={`inline-flex items-center ${isConfigured ? "cursor-pointer" : "cursor-not-allowed opacity-40"}`}
                title={t("settings.models.cloudTranscription.selectButton")}
              >
                <input
                  type="checkbox"
                  className="sr-only peer"
                  checked={isActive}
                  disabled={!isConfigured}
                  onChange={() => { if (isConfigured && !isActive) onSelect("cloud"); }}
                />
                <div className="relative w-11 h-6 bg-mid-gray/20 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-logo-primary rounded-full peer peer-checked:after:translate-x-full rtl:peer-checked:after:-translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:start-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-background-ui peer-disabled:opacity-50" />
              </label>
            </div>
          </div>
        </>
      )}
    </div>
  );
};
