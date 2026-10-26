import React, { useMemo } from "react";
import { useTranslation } from "react-i18next";
import type { LanguageDetectionMode } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { useLanguageDetector } from "@/hooks/useLanguageDetector";
import { Dropdown, type DropdownOption } from "@/components/ui/Dropdown";
import { SettingContainer } from "@/components/ui/SettingContainer";

interface LanguageDetectionProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const LanguageDetection: React.FC<LanguageDetectionProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const mode = getSetting("language_detection") ?? "when_needed";
  const detector = useLanguageDetector();

  const options = useMemo<DropdownOption[]>(
    () => [
      {
        value: "when_needed",
        label: t("settings.advanced.languageDetection.options.whenNeeded"),
      },
      {
        value: "always",
        label: t("settings.advanced.languageDetection.options.always"),
      },
    ],
    [t],
  );

  const handleSelect = async (value: string) => {
    await updateSetting("language_detection", value as LanguageDetectionMode);
    // Choosing Always is the consent to fetch the detector it needs.
    if (value === "always" && detector.downloaded === false) {
      await detector.download();
    }
  };

  return (
    <SettingContainer
      title={t("settings.advanced.languageDetection.title")}
      description={t("settings.advanced.languageDetection.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
      layout="horizontal"
    >
      <div className="flex flex-col items-end">
        <Dropdown
          options={options}
          selectedValue={mode}
          onSelect={handleSelect}
          disabled={isUpdating("language_detection")}
        />
        {mode === "always" && detector.downloaded === false && (
          <p className="mt-[4px] mb-0 max-w-[240px] text-end text-small text-ink-3">
            {detector.downloading ? (
              <span className="tabular-nums whitespace-nowrap">
                {t("modelSelector.downloading", {
                  percentage: detector.percent,
                })}
              </span>
            ) : (
              <>
                {t("settings.advanced.languageDetection.detectorMissing")}{" "}
                <button
                  type="button"
                  onClick={detector.download}
                  className="cursor-pointer text-text underline decoration-line-strong underline-offset-[3px] hover:decoration-text focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
                >
                  {t("settings.general.language.detectorDownload")}
                </button>
              </>
            )}
          </p>
        )}
      </div>
    </SettingContainer>
  );
};
