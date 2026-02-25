import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, ChevronUp } from "lucide-react";
import { commands } from "@/bindings";
import { MODEL_ID_GEMINI } from "@/lib/constants/models";
import Badge from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { Select } from "@/components/ui/Select";

type TestStatus = "idle" | "testing" | "ok" | "error";

const GEMINI_PRESET_MODELS = [
  "gemini-3-flash-preview",
  "gemini-2.5-flash",
  "gemini-2.5-flash-lite",
] as const;

interface GeminiTranscriptionCardProps {
  isActive: boolean;
  onSelect: (modelId: string) => void;
}

export const GeminiTranscriptionCard: React.FC<GeminiTranscriptionCardProps> = ({
  isActive,
  onSelect,
}) => {
  const { t } = useTranslation();
  const [isExpanded, setIsExpanded] = useState(false);
  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState("gemini-2.5-flash");
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
        setApiKey(s.gemini_api_key ?? "");
        setModel(s.gemini_model ?? "gemini-2.5-flash");
      }
    });
  }, []);

  useEffect(() => {
    if (isActive) setIsExpanded(true);
  }, [isActive]);

  useEffect(
    () => () => {
      if (okTimerRef.current) clearTimeout(okTimerRef.current);
    },
    [],
  );

  const isConfigured = apiKey.trim() !== "" && model.trim() !== "";

  const save = async (fn: () => Promise<unknown>) => {
    setIsSaving(true);
    try {
      await fn();
    } catch (e) {
      console.error("Failed to save Gemini setting:", e);
    } finally {
      setIsSaving(false);
    }
  };

  const handleTest = async () => {
    if (okTimerRef.current) clearTimeout(okTimerRef.current);
    setTestStatus("testing");
    setTestError(null);
    const result = await commands.testGeminiConnection();
    if (result.status === "ok") {
      setTestStatus("ok");
      okTimerRef.current = setTimeout(() => setTestStatus("idle"), 2000);
    } else {
      setTestStatus("error");
      setTestError(result.error ?? t("settings.models.gemini.testFailed"));
    }
  };

  function getTestLabel(): string {
    switch (testStatus) {
      case "ok":
        return "\u2713";
      case "error":
        return "\u2717";
      default:
        return t("settings.models.gemini.test");
    }
  }

  function renderBadge() {
    if (isActive) {
      return <Badge variant="primary">{t("modelSelector.active")}</Badge>;
    }
    const labelKey = isConfigured
      ? "settings.models.gemini.configured"
      : "settings.models.gemini.notConfigured";
    return <Badge variant="secondary">{t(labelKey)}</Badge>;
  }

  const borderClass = isActive
    ? "border-logo-primary/50 bg-logo-primary/10"
    : "border-mid-gray/20 hover:border-logo-primary/30";

  return (
    <div
      className={`flex flex-col rounded-xl px-4 py-3 gap-2 border-2 transition-all duration-200 ${borderClass}`}
    >
      {/* Header */}
      <button
        type="button"
        className="flex items-start justify-between w-full text-left"
        onClick={() => setIsExpanded((v) => !v)}
      >
        <div className="flex flex-col items-start flex-1 min-w-0">
          <div className="flex items-center gap-3 flex-wrap">
            <h3 className="text-base font-semibold text-text">
              {t("settings.models.gemini.title")}
            </h3>
            {renderBadge()}
          </div>
          <p className="text-sm text-text/60 leading-relaxed">
            {t("settings.models.gemini.description")}
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
            {/* API Key */}
            <div className="flex flex-col gap-1">
              <label className="text-xs font-medium text-text/60">
                {t("settings.models.gemini.apiKeyLabel")}
              </label>
              <Input
                type="password"
                variant="compact"
                value={apiKey}
                onChange={(e) => {
                  setApiKey(e.target.value);
                  setTestStatus("idle");
                }}
                onBlur={(e) =>
                  void save(() => commands.changeGeminiApiKey(e.target.value))
                }
                placeholder={t("settings.models.gemini.apiKeyPlaceholder")}
                className="w-full"
                disabled={isSaving}
              />
              <p className="text-xs text-text/30">
                {t("settings.models.gemini.apiKeyHint")}
              </p>
              {testStatus === "error" && (
                <p className="text-xs text-red-400 break-all">{testError}</p>
              )}
            </div>

            {/* Model */}
            <div className="flex flex-col gap-1">
              <label className="text-xs font-medium text-text/60">
                {t("settings.models.gemini.modelLabel")}
              </label>
              <Select
                value={model || null}
                options={GEMINI_PRESET_MODELS.map((m) => ({ value: m, label: m }))}
                onChange={(val) => {
                  const v = val ?? "";
                  setModel(v);
                  void save(() => commands.changeGeminiModel(v));
                }}
                onCreateOption={(val) => {
                  setModel(val);
                  void save(() => commands.changeGeminiModel(val));
                }}
                placeholder={t("settings.models.gemini.modelPlaceholder")}
                disabled={isSaving}
                isClearable={false}
                isCreatable
                formatCreateLabel={(input) => `Use "${input}"`}
              />
            </div>

            {/* Bottom row: Activate + Test */}
            <div className="flex items-center justify-end gap-2">
              {!isActive && (
                <Button
                  variant="primary"
                  size="sm"
                  onClick={() => {
                    if (isConfigured) onSelect(MODEL_ID_GEMINI);
                  }}
                  disabled={!isConfigured}
                >
                  {t("settings.models.gemini.selectButton")}
                </Button>
              )}
              <Button
                variant="secondary"
                size="sm"
                onClick={() => void handleTest()}
                disabled={!isConfigured || testStatus === "testing"}
                className={[
                  "w-16 justify-center shrink-0 transition-colors",
                  testStatus === "ok" ? "!text-green-500" : "",
                  testStatus === "error" ? "!text-red-400" : "",
                ].join(" ")}
              >
                {getTestLabel()}
              </Button>
            </div>
          </div>
        </>
      )}
    </div>
  );
};
