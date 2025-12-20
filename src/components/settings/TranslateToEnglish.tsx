import React, { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";
import { useModels } from "../../hooks/useModels";

interface TranslateToEnglishProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const TranslateToEnglish: React.FC<TranslateToEnglishProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const { currentModel, models } = useModels();

    const translateToEnglish = getSetting("translate_to_english") || false;
    const currentModelInfo = models.find((m) => m.id === currentModel);
    const isDisabledTranslation = currentModelInfo
      ? !currentModelInfo.supports_translation
      : false;

    const description = useMemo(() => {
      if (isDisabledTranslation) {
        const currentModelDisplayName = models.find(
          (model) => model.id === currentModel,
        )?.name;
        return t(
          "settings.advanced.translateToEnglish.descriptionUnsupported",
          {
            model: currentModelDisplayName,
          },
        );
      }

      return t("settings.advanced.translateToEnglish.description");
    }, [t, models, currentModel, isDisabledTranslation]);

    return (
      <ToggleSwitch
        checked={translateToEnglish}
        onChange={(enabled) => updateSetting("translate_to_english", enabled)}
        isUpdating={isUpdating("translate_to_english")}
        disabled={isDisabledTranslation}
        label={t("settings.advanced.translateToEnglish.label")}
        description={description}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);
