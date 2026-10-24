import React from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../ui/Button";
import { Dropdown } from "../../ui/Dropdown";
import { SettingContainer } from "../../ui/SettingContainer";
import {
  STEP_ORDER,
  type OnboardingStep,
} from "../../onboarding/onboardingStore";

interface OnboardingPreviewProps {
  onPreview: (step: OnboardingStep) => void;
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
        onSelect={(value) => onPreview(value as OnboardingStep)}
      />
    </SettingContainer>
  );
};

interface DebugRunProps {
  /**
   * `onboardingRun`: the real onboarding from Welcome (permission prompts,
   * downloads, the practice note and setting writes), exactly as a new person
   * gets it. `modelMigration`: the real model migration screen, which needs
   * old-format models on disk to have anything to show.
   */
  run: "onboardingRun" | "modelMigration";
  onRun: () => void;
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/** Runs a real onboarding flow, not a preview. */
export const DebugRun: React.FC<DebugRunProps> = ({
  run,
  onRun,
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();

  return (
    <SettingContainer
      title={t(`settings.debug.${run}.title`)}
      description={t(`settings.debug.${run}.description`)}
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <Button variant="secondary" size="sm" onClick={onRun}>
        {t(`settings.debug.${run}.button`)}
      </Button>
    </SettingContainer>
  );
};
