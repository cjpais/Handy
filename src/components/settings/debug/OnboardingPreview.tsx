import React from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../ui/Button";
import { SettingContainer } from "../../ui/SettingContainer";

export type OnboardingPreviewStep = "accessibility" | "model";

interface OnboardingPreviewProps {
  onPreview: (step: OnboardingPreviewStep) => void;
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const OnboardingPreview: React.FC<OnboardingPreviewProps> = ({
  onPreview,
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();

  return (
    <SettingContainer
      title={t("settings.debug.onboardingPreview.title")}
      description={t("settings.debug.onboardingPreview.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <div className="flex gap-2">
        <Button
          variant="secondary"
          size="md"
          onClick={() => onPreview("accessibility")}
        >
          {t("settings.debug.onboardingPreview.permissionsButton")}
        </Button>
        <Button
          variant="secondary"
          size="md"
          onClick={() => onPreview("model")}
        >
          {t("settings.debug.onboardingPreview.modelsButton")}
        </Button>
      </div>
    </SettingContainer>
  );
};
