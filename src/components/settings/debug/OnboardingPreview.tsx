import React from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../ui/Button";
import { Dropdown } from "../../ui/Dropdown";
import { SettingContainer } from "../../ui/SettingContainer";
import {
  STEP_ORDER,
  type OnboardingStep,
} from "../../onboarding/onboardingStore";

export type OnboardingPreviewStep = OnboardingStep;

interface OnboardingPreviewProps {
  onPreview: (step: OnboardingPreviewStep) => void;
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/** Opens onboarding at any screen, inert, to look at it. */
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
      <Dropdown
        options={STEP_ORDER.map((step) => ({
          value: step,
          label: t(`settings.debug.onboardingPreview.screens.${step}`),
        }))}
        selectedValue={null}
        placeholder={t("settings.debug.onboardingPreview.choose")}
        onSelect={(value) => onPreview(value as OnboardingPreviewStep)}
      />
    </SettingContainer>
  );
};

interface OnboardingRunProps {
  onRun: () => void;
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/**
 * Runs the real onboarding from Welcome: permission prompts, downloads, the
 * practice note and setting writes, exactly as a new person gets it.
 */
export const OnboardingRun: React.FC<OnboardingRunProps> = ({
  onRun,
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();

  return (
    <SettingContainer
      title={t("settings.debug.onboardingRun.title")}
      description={t("settings.debug.onboardingRun.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <Button variant="secondary" size="sm" onClick={onRun}>
        {t("settings.debug.onboardingRun.button")}
      </Button>
    </SettingContainer>
  );
};

interface ModelMigrationRunProps {
  onRun: () => void;
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/**
 * Opens the real model migration screen. It needs old-format models on disk
 * to have anything to show.
 */
export const ModelMigrationRun: React.FC<ModelMigrationRunProps> = ({
  onRun,
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();

  return (
    <SettingContainer
      title={t("settings.debug.modelMigration.title")}
      description={t("settings.debug.modelMigration.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <Button variant="secondary" size="sm" onClick={onRun}>
        {t("settings.debug.modelMigration.button")}
      </Button>
    </SettingContainer>
  );
};
