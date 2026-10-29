import React from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { SettingContainer } from "../ui/SettingContainer";
import { Button } from "../ui/Button";
import { useSettings } from "../../hooks/useSettings";

interface SinglishVocabularyProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const SinglishVocabulary: React.FC<SinglishVocabularyProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("singlish_vocabulary_enabled") ?? true;

    const handleOpen = async () => {
      try {
        await commands.openSinglishVocabularyFile();
      } catch (error) {
        console.error("Failed to open Singlish vocabulary file:", error);
      }
    };

    return (
      <>
        <ToggleSwitch
          checked={enabled}
          onChange={(value) =>
            updateSetting("singlish_vocabulary_enabled", value)
          }
          isUpdating={isUpdating("singlish_vocabulary_enabled")}
          label={t("settings.advanced.singlishVocabulary.label")}
          description={t("settings.advanced.singlishVocabulary.description")}
          descriptionMode={descriptionMode}
          grouped={grouped}
        />
        <SettingContainer
          title={t("settings.advanced.singlishVocabulary.editTitle")}
          description={t(
            "settings.advanced.singlishVocabulary.editDescription",
          )}
          descriptionMode={descriptionMode}
          grouped={grouped}
        >
          <Button variant="secondary" size="sm" onClick={handleOpen}>
            {t("settings.advanced.singlishVocabulary.openFile")}
          </Button>
        </SettingContainer>
      </>
    );
  },
);
