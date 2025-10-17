import React, { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ModelInfo } from "../../lib/types";
import ModelCard from "./ModelCard";

interface OnboardingProps {
  onModelSelected: () => void;
}

const Onboarding: React.FC<OnboardingProps> = ({ onModelSelected }) => {
  const [availableModels, setAvailableModels] = useState<ModelInfo[]>([]);
  const [downloading, setDownloading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    loadModels();
  }, []);

  const loadModels = async () => {
    try {
      const models: ModelInfo[] = await invoke("get_available_models");
      // Only show downloadable models for onboarding
      setAvailableModels(models.filter((m) => !m.is_downloaded));
    } catch (err) {
      console.error("Failed to load models:", err);
      setError("Failed to load available models");
    }
  };

  const handleDownloadModel = async (modelId: string) => {
    setDownloading(true);
    setError(null);

    // Immediately transition to main app - download will continue in footer
    onModelSelected();

    try {
      await invoke("download_model", { modelId });
    } catch (err) {
      console.error("Download failed:", err);
      setError(`Failed to download model: ${err}`);
      setDownloading(false);
    }
  };

  const getRecommendedBadge = (modelId: string): boolean => {
    return modelId === "parakeet-tdt-0.6b-v3";
  };

  return (
    <div className="max-w-4xl mx-auto text-center space-y-6">
      <p className="text-text/70 max-w-md font-medium mx-auto">
        To get started, choose a transcription model
      </p>

      {error && (
        <div className="bg-red-500/10 border border-red-500/20 rounded-lg p-4">
          <p className="text-red-400 text-sm">{error}</p>
        </div>
      )}

      <div className="max-w-2xl mx-auto mt-4">
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
          {availableModels
            .filter((model) => getRecommendedBadge(model.id))
            .map((model) => (
              <ModelCard
                key={model.id}
                model={model}
                variant="featured"
                badgeText="Recommended"
                disabled={downloading}
                className="sm:col-span-2"
                onSelect={handleDownloadModel}
              />
            ))}

          {availableModels
            .filter((model) => !getRecommendedBadge(model.id))
            .sort((a, b) => a.size_mb - b.size_mb)
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
