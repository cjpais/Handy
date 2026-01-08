import React, { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { commands, type ModelInfo } from "@/bindings";
import { listen } from "@tauri-apps/api/event";
import ModelCard from "./ModelCard";
import KBVETextLogo from "../icons/KBVETextLogo";
import { Button } from "../ui/Button";

interface VadDownloadProgress {
  downloaded: number;
  total: number;
  percentage: number;
}

type OnboardingStep = "vad" | "transcription";

interface OnboardingProps {
  onModelSelected: () => void;
}

const Onboarding: React.FC<OnboardingProps> = ({ onModelSelected }) => {
  const { t } = useTranslation();
  const [step, setStep] = useState<OnboardingStep>("vad");
  const [availableModels, setAvailableModels] = useState<ModelInfo[]>([]);
  const [downloading, setDownloading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [vadReady, setVadReady] = useState(false);
  const [vadDownloading, setVadDownloading] = useState(false);
  const [vadProgress, setVadProgress] = useState(0);

  useEffect(() => {
    // Check if VAD model is already ready
    checkVadStatus();

    // Listen for VAD download progress
    const unlistenProgress = listen<VadDownloadProgress>(
      "vad-download-progress",
      (event) => {
        setVadProgress(Math.round(event.payload.percentage));
      }
    );

    const unlistenComplete = listen("vad-download-complete", () => {
      setVadDownloading(false);
      setVadReady(true);
      setVadProgress(100);
    });

    return () => {
      unlistenProgress.then((fn) => fn());
      unlistenComplete.then((fn) => fn());
    };
  }, []);

  const checkVadStatus = async () => {
    try {
      const isReady = await commands.isVadModelReady();
      if (isReady) {
        setVadReady(true);
        setStep("transcription");
        loadModels();
      }
    } catch (err) {
      console.error("Failed to check VAD status:", err);
    }
  };

  const handleDownloadVad = async () => {
    setVadDownloading(true);
    setError(null);
    setVadProgress(0);

    try {
      const result = await commands.downloadVadModelIfNeeded();
      if (result.status === "ok") {
        setVadReady(true);
        setStep("transcription");
        loadModels();
      } else {
        setError(t("onboarding.errors.vadDownload", { error: result.error }));
      }
    } catch (err) {
      console.error("VAD download failed:", err);
      setError(t("onboarding.errors.vadDownload", { error: String(err) }));
    } finally {
      setVadDownloading(false);
    }
  };

  const handleSkipToTranscription = () => {
    // Allow skipping if VAD is bundled with the app
    setStep("transcription");
    loadModels();
  };

  const loadModels = async () => {
    try {
      const result = await commands.getAvailableModels();
      if (result.status === "ok") {
        // Only show downloadable models for onboarding
        setAvailableModels(result.data.filter((m) => !m.is_downloaded));
      } else {
        setError(t("onboarding.errors.loadModels"));
      }
    } catch (err) {
      console.error("Failed to load models:", err);
      setError(t("onboarding.errors.loadModels"));
    }
  };

  const handleDownloadModel = async (modelId: string) => {
    setDownloading(true);
    setError(null);

    // Immediately transition to main app - download will continue in footer
    onModelSelected();

    try {
      const result = await commands.downloadModel(modelId);
      if (result.status === "error") {
        console.error("Download failed:", result.error);
        setError(t("onboarding.errors.downloadModel", { error: result.error }));
        setDownloading(false);
      }
    } catch (err) {
      console.error("Download failed:", err);
      setError(t("onboarding.errors.downloadModel", { error: String(err) }));
      setDownloading(false);
    }
  };

  const getRecommendedBadge = (modelId: string): boolean => {
    return modelId === "parakeet-tdt-0.6b-v3";
  };

  return (
    <div className="h-screen w-screen flex flex-col p-6 gap-4 inset-0">
      <div className="flex flex-col items-center gap-2 shrink-0">
        <KBVETextLogo width={200} />
        <p className="text-text/70 max-w-md font-medium mx-auto">
          {step === "vad"
            ? t("onboarding.vadSubtitle")
            : t("onboarding.subtitle")}
        </p>
      </div>

      <div className="max-w-[600px] w-full mx-auto text-center flex-1 flex flex-col min-h-0">
        {error && (
          <div className="bg-red-500/10 border border-red-500/20 rounded-lg p-4 mb-4 shrink-0">
            <p className="text-red-400 text-sm">{error}</p>
          </div>
        )}

        {step === "vad" && (
          <div className="flex flex-col gap-6 items-center justify-center flex-1">
            <div className="bg-background-dark/50 rounded-2xl p-6 w-full max-w-md">
              <h2 className="text-lg font-semibold mb-2">
                {t("onboarding.vad.title")}
              </h2>
              <p className="text-text/70 text-sm mb-6">
                {t("onboarding.vad.description")}
              </p>

              {vadDownloading ? (
                <div className="flex flex-col gap-3">
                  <div className="w-full bg-mid-gray/30 rounded-full h-2">
                    <div
                      className="bg-logo-primary h-2 rounded-full transition-all duration-300"
                      style={{ width: `${vadProgress}%` }}
                    />
                  </div>
                  <p className="text-sm text-text/60">
                    {t("onboarding.vad.downloading", { progress: vadProgress })}
                  </p>
                </div>
              ) : vadReady ? (
                <div className="flex flex-col gap-3">
                  <p className="text-green-400 text-sm">
                    {t("onboarding.vad.ready")}
                  </p>
                  <Button
                    onClick={() => {
                      setStep("transcription");
                      loadModels();
                    }}
                  >
                    {t("onboarding.vad.continue")}
                  </Button>
                </div>
              ) : (
                <Button onClick={handleDownloadVad}>
                  {t("onboarding.vad.download")}
                </Button>
              )}
            </div>

            <p className="text-text/40 text-xs">
              {t("onboarding.vad.size")}
            </p>
          </div>
        )}

        {step === "transcription" && (
          <div className="flex flex-col gap-4">
            {availableModels
              .filter((model) => getRecommendedBadge(model.id))
              .map((model) => (
                <ModelCard
                  key={model.id}
                  model={model}
                  variant="featured"
                  disabled={downloading}
                  onSelect={handleDownloadModel}
                />
              ))}

            {availableModels
              .filter((model) => !getRecommendedBadge(model.id))
              .sort((a, b) => Number(a.size_mb) - Number(b.size_mb))
              .map((model) => (
                <ModelCard
                  key={model.id}
                  model={model}
                  disabled={downloading}
                  onSelect={handleDownloadModel}
                />
              ))}
          </div>
        )}
      </div>
    </div>
  );
};

export default Onboarding;
