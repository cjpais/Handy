import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import Onboarding, { AccessibilityOnboarding } from "../../onboarding";
import { Button } from "../../ui/Button";
import { SettingContainer } from "../../ui/SettingContainer";

type OnboardingPreviewStep = "accessibility" | "model";

// Stable identity so the previewed component's effects don't re-run (and reset
// their state) on every parent render.
const NOOP = () => {};

interface OnboardingPreviewProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const OnboardingPreview: React.FC<OnboardingPreviewProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const [step, setStep] = useState<OnboardingPreviewStep | null>(null);

  return (
    <>
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
            onClick={() => setStep("accessibility")}
          >
            {t("settings.debug.onboardingPreview.permissionsButton")}
          </Button>
          <Button
            variant="secondary"
            size="md"
            onClick={() => setStep("model")}
          >
            {t("settings.debug.onboardingPreview.modelsButton")}
          </Button>
        </div>
      </SettingContainer>

      {/* The onboarding screens are full-window, so cover the settings UI
          rather than nesting them in a dialog — the point of the preview is to
          see them laid out exactly as a first-run user would. */}
      {step && (
        <div className="fixed inset-0 z-50 bg-background">
          {step === "accessibility" ? (
            <AccessibilityOnboarding onComplete={NOOP} preview />
          ) : (
            <Onboarding onModelSelected={NOOP} preview />
          )}
          <button
            type="button"
            onClick={() => setStep(null)}
            className="absolute top-4 end-4 z-10 rounded-lg border border-mid-gray/20 bg-background px-4 py-2 text-sm font-medium text-text shadow-lg hover:bg-background-ui/30 cursor-pointer"
          >
            {t("settings.debug.onboardingPreview.exitButton")}
          </button>
        </div>
      )}
    </>
  );
};
