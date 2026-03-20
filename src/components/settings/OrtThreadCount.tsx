import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "../../hooks/useSettings";
import { Button } from "../ui/Button";
import { Input } from "../ui/Input";
import { SettingContainer } from "../ui/SettingContainer";
import { OrtAutoTuneModal } from "./OrtAutoTuneModal";

interface OrtThreadCountProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const OrtThreadCount: React.FC<OrtThreadCountProps> = ({
  descriptionMode = "inline",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const [modalOpen, setModalOpen] = useState(false);

  const threadCount = getSetting("ort_thread_count") ?? 0;

  const handleChange = async (event: React.ChangeEvent<HTMLInputElement>) => {
    const value = parseInt(event.target.value, 10);
    if (!isNaN(value) && value >= 0 && value <= 32) {
      updateSetting("ort_thread_count", value);
    }
  };

  const handleApply = (threadCount: number) => {
    updateSetting("ort_thread_count", threadCount);
  };

  return (
    <>
      <SettingContainer
        title={t("settings.advanced.ortThreadCount.title")}
        description={t("settings.advanced.ortThreadCount.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
        layout="horizontal"
      >
        <div className="flex items-center space-x-2">
          <Input
            type="number"
            min="0"
            max="32"
            value={threadCount}
            onChange={handleChange}
            disabled={isUpdating("ort_thread_count")}
            className="w-20"
          />
          {threadCount === 0 && (
            <span className="text-sm text-text">
              {t("settings.advanced.ortThreadCount.auto")}
            </span>
          )}
          <Button
            variant="secondary"
            size="sm"
            onClick={() => setModalOpen(true)}
          >
            {t("settings.advanced.ortThreadCount.autoButton")}
          </Button>
        </div>
      </SettingContainer>
      <OrtAutoTuneModal
        isOpen={modalOpen}
        onClose={() => setModalOpen(false)}
        onApply={handleApply}
      />
    </>
  );
};
