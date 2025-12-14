import React, { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { commands, type ModelInfo } from "@/bindings";
import ModelCard from "./ModelCard";
import HandyTextLogo from "../icons/HandyTextLogo";

interface OnboardingProps {
  onModelSelected: () => void;
}

const Onboarding: React.FC<OnboardingProps> = ({ onModelSelected }) => {
  const { t } = useTranslation();
  const [availableModels, setAvailableModels] = useState<ModelInfo[]>([]);
  const [downloading, setDownloading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    loadModels();
  }, []);

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
        <HandyTextLogo width={200} />
        <p className="text-text/70 max-w-md font-medium mx-auto">
          {t("onboarding.subtitle")}
        </p>
      </div>

      <div className="max-w-[600px] w-full mx-auto text-center flex-1 flex flex-col min-h-0">
        {error && (
          <div className="bg-red-500/10 border border-red-500/20 rounded-lg p-4 mb-4 shrink-0">
            <p className="text-red-400 text-sm">{error}</p>
          </div>
        )}

        {/*<div className="flex flex-col gap-4 bg-background-dark p-4 py-5 w-full rounded-2xl flex-1 overflow-y-auto min-h-0">*/}
        <div className="flex flex-col gap-4 ">
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
      </div>
    </div>
  );
};

export default Onboarding;
