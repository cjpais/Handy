import React from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "@/hooks/useSettings";
import { useLanguageDetector } from "@/hooks/useLanguageDetector";
import { SettingContainer } from "@/components/ui/SettingContainer";
import { Switch } from "@/components/ui/Switch";
import { LanguageDetectorDownload } from "./LanguageDetectorDownload";

interface LanguageDetectionProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

/** Use the language detector for every model, so Auto only picks from their languages. */
export const LanguageDetection: React.FC<LanguageDetectionProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const mode = getSetting("language_detection") ?? "when_needed";
  const detector = useLanguageDetector();
  const missing = detector.downloaded === false;
  // Without the detector nothing is limited, so it shows off unless the
  // detector is on its way.
  const checked = mode === "always" && (!missing || detector.downloading);
  const label = t("settings.advanced.languageDetection.title");

  const handleChange = async (on: boolean) => {
    // Turning it off mid-download is how it's stopped.
    if (!on && detector.downloading) {
      detector.cancel();
    }
    await updateSetting("language_detection", on ? "always" : "when_needed");
    // Turning it on is the consent to fetch the detector it needs.
    if (on && missing) {
      await detector.download();
    }
  };

  return (
    <SettingContainer
      title={label}
      description={t("settings.advanced.languageDetection.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <div className="flex items-center gap-3">
        {missing && (
          <LanguageDetectorDownload
            downloading={detector.downloading}
            percent={detector.percent}
            hint={t(
              detector.downloading
                ? "settings.advanced.languageDetection.downloadingHint"
                : "settings.advanced.languageDetection.downloadHint",
            )}
          />
        )}
        <Switch
          checked={checked}
          onChange={handleChange}
          disabled={isUpdating("language_detection")}
          ariaLabel={label}
        />
      </div>
    </SettingContainer>
  );
};
