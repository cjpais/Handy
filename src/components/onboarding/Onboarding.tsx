import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import type { ModelInfo } from "@/bindings";
import ModelCard from "./ModelCard";
import HandyTextLogo from "../icons/HandyTextLogo";
import { useModelStore } from "../../stores/modelStore";

interface OnboardingProps {
  onModelSelected: () => void;
}

const Onboarding: React.FC<OnboardingProps> = ({ onModelSelected }) => {
  const { t } = useTranslation();
  const { models, downloadModel, error: modelError } = useModelStore();
  const [downloading, setDownloading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Only show downloadable models for onboarding
  const availableModels = models.filter((m: ModelInfo) => !m.is_downloaded);

  const handleDownloadModel = async (modelId: string) => {
    setDownloading(true);
    setError(null);

    // Start the download (updates Zustand store)
    const downloadPromise = downloadModel(modelId);

    // Immediately transition to main app - download will continue in footer
    onModelSelected();

    // Note: We don't await or handle the result here since the component
    // will unmount. The Zustand store handles download state, and any errors
    // will be visible in the main app's ModelSelector.
    downloadPromise.catch((err: Error) => {
      console.error("Download failed:", err);
    });
  };

  const isRecommendedModel = (model: ModelInfo): boolean => {
    return model.is_recommended;
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

        <div className="flex flex-col gap-4 pb-6">
          {availableModels
            .filter((model: ModelInfo) => isRecommendedModel(model))
            .map((model: ModelInfo) => (
              <ModelCard
                key={model.id}
                model={model}
                variant="featured"
                disabled={downloading}
                onSelect={handleDownloadModel}
              />
            ))}

          {availableModels
            .filter((model: ModelInfo) => !isRecommendedModel(model))
            .sort(
              (a: ModelInfo, b: ModelInfo) =>
                Number(a.size_mb) - Number(b.size_mb),
            )
            .map((model: ModelInfo) => (
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
