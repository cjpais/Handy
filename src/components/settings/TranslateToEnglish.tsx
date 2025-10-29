import React, { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";
import { useModels } from "../../hooks/useModels";

interface TranslateToEnglishProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

const unsupportedTranslationModels: Record<string, { description: string }> = {
  "parakeet-tdt-0.6b-v3": {
    description: "Translation is not supported by the Parakeet model.",
  },
  turbo: {
    description: "Translation is not supported by the Whisper Turbo model.",
  },
};

export const TranslateToEnglish: React.FC<TranslateToEnglishProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const { currentModel, loadCurrentModel } = useModels();

    const translateToEnglish = getSetting("translate_to_english") || false;
    const isDisabledTranslation = currentModel in unsupportedTranslationModels;
    const description = isDisabledTranslation
      ? unsupportedTranslationModels[currentModel].description
      : "Automatically translate speech from other languages to English during transcription.";

    // Listen for model state changes to update UI reactively
    useEffect(() => {
      const modelStateUnlisten = listen("model-state-changed", () => {
        loadCurrentModel();
      });

      return () => {
        modelStateUnlisten.then((fn) => fn());
      };
    }, [loadCurrentModel]);

    return (
      <ToggleSwitch
        checked={translateToEnglish}
        onChange={(enabled) => updateSetting("translate_to_english", enabled)}
        isUpdating={isUpdating("translate_to_english")}
        disabled={isDisabledTranslation}
        label="Translate to English"
        description={description}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);
