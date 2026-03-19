import React from "react";
import { useTranslation } from "react-i18next";
import { Slider } from "../../ui/Slider";
import { useSettings } from "../../../hooks/useSettings";

interface RecordingBufferProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const RecordingBuffer: React.FC<RecordingBufferProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { settings, updateSetting } = useSettings();

  const handleBufferChange = (value: number) => {
    updateSetting("extra_recording_buffer_ms", value);
  };

  return (
    <Slider
      value={settings?.extra_recording_buffer_ms ?? 0}
      onChange={handleBufferChange}
      min={0}
      max={1500}
      step={50}
      label={t("settings.debug.recordingBuffer.title")}
      description={t("settings.debug.recordingBuffer.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
      formatValue={(v) => `${v}ms`}
    />
  );
};
