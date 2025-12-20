import React, { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ask } from "@tauri-apps/plugin-dialog";
import { Globe, Languages } from "lucide-react";
import type { ModelCardStatus } from "../../onboarding";
import { ModelCard } from "../../onboarding";
import { useModels } from "../../../hooks/useModels";

type ModelFilter = "all" | "multiLanguage" | "translation";

export const ModelsSettings: React.FC = () => {
  const { t } = useTranslation();
  const [activeFilter, setActiveFilter] = useState<ModelFilter>("all");
  const [switchingModelId, setSwitchingModelId] = useState<string | null>(null);
  const {
    models,
    currentModel,
    downloadingModels,
    downloadProgress,
    downloadStats,
    extractingModels,
    loading,
    downloadModel,
    selectModel,
    deleteModel,
  } = useModels();

  const getModelStatus = (modelId: string): ModelCardStatus => {
    if (extractingModels.has(modelId)) {
      return "extracting";
    }
    if (downloadingModels.has(modelId)) {
      return "downloading";
    }
    if (switchingModelId === modelId) {
      return "switching";
    }
    if (modelId === currentModel) {
      return "active";
    }
    const model = models.find((m) => m.id === modelId);
    if (model?.is_downloaded) {
      return "available";
    }
    return "downloadable";
  };

  const getDownloadProgress = (modelId: string): number | undefined => {
    const progress = downloadProgress.get(modelId);
    return progress?.percentage;
  };

  const getDownloadSpeed = (modelId: string): number | undefined => {
    const stats = downloadStats.get(modelId);
    return stats?.speed;
  };

  const handleModelSelect = async (modelId: string) => {
    setSwitchingModelId(modelId);
    try {
      await selectModel(modelId);
    } finally {
      setSwitchingModelId(null);
    }
  };

  const handleModelDownload = async (modelId: string) => {
    await downloadModel(modelId);
  };

  const handleModelDelete = async (modelId: string) => {
    const model = models.find((m) => m.id === modelId);
    const modelName = model?.name || modelId;

    const confirmed = await ask(
      t("settings.models.deleteConfirm", { modelName }),
      {
        title: t("settings.models.deleteTitle"),
        kind: "warning",
      },
    );

    if (confirmed) {
      try {
        await deleteModel(modelId);
      } catch (err) {
        console.error(`Failed to delete model ${modelId}:`, err);
      }
    }
  };

  // Filter models based on active filter
  const filteredModels = useMemo(() => {
    return models.filter((model) => {
      switch (activeFilter) {
        case "multiLanguage":
          return model.supports_language_selection;
        case "translation":
          return model.supports_translation;
        default:
          return true;
      }
    });
  }, [models, activeFilter]);

  if (loading) {
    return (
      <div className="max-w-3xl w-full mx-auto">
        <div className="flex items-center justify-center py-16">
          <div className="w-8 h-8 border-2 border-logo-primary border-t-transparent rounded-full animate-spin" />
        </div>
      </div>
    );
  }

  return (
    <div className="max-w-3xl w-full mx-auto space-y-4">
      <div className="mb-6">
        <h1 className="text-xl font-semibold mb-2">
          {t("settings.models.title")}
        </h1>
        <p className="text-sm text-text/60">
          {t("settings.models.description")}
        </p>
      </div>
      <div className="flex gap-2 mb-4">
        <button
          type="button"
          onClick={() => setActiveFilter("all")}
          className={`px-3 py-1.5 text-sm font-medium rounded-lg transition-colors ${
            activeFilter === "all"
              ? "bg-logo-primary/20 text-logo-primary"
              : "bg-mid-gray/10 text-text/60 hover:bg-mid-gray/20"
          }`}
        >
          {t("settings.models.filters.all")}
        </button>
        <button
          type="button"
          onClick={() => setActiveFilter("multiLanguage")}
          className={`flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-lg transition-colors ${
            activeFilter === "multiLanguage"
              ? "bg-logo-primary/20 text-logo-primary"
              : "bg-mid-gray/10 text-text/60 hover:bg-mid-gray/20"
          }`}
        >
          <Globe className="w-3.5 h-3.5" />
          {t("settings.models.filters.multiLanguage")}
        </button>
        <button
          type="button"
          onClick={() => setActiveFilter("translation")}
          className={`flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-lg transition-colors ${
            activeFilter === "translation"
              ? "bg-logo-primary/20 text-logo-primary"
              : "bg-mid-gray/10 text-text/60 hover:bg-mid-gray/20"
          }`}
        >
          <Languages className="w-3.5 h-3.5" />
          {t("settings.models.filters.translation")}
        </button>
      </div>
      {filteredModels.length > 0 ? (
        <div className="space-y-3">
          {filteredModels.map((model) => (
            <ModelCard
              key={model.id}
              model={model}
              status={getModelStatus(model.id)}
              variant={model.is_recommended ? "featured" : "default"}
              onSelect={handleModelSelect}
              onDownload={handleModelDownload}
              onDelete={handleModelDelete}
              downloadProgress={getDownloadProgress(model.id)}
              downloadSpeed={getDownloadSpeed(model.id)}
            />
          ))}
        </div>
      ) : (
        <div className="text-center py-8 text-text/50">
          {t("settings.models.noModelsMatch")}
        </div>
      )}
    </div>
  );
};
