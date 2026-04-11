import React from "react";
import { useTranslation } from "react-i18next";
import { Slider } from "../ui/Slider";
import { useSettings } from "../../hooks/useSettings";

interface TypedInputDelayProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const TypedInputDelay: React.FC<TypedInputDelayProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { settings, getSetting, updateSetting } = useSettings();

  if (getSetting("paste_method") !== "type") {
    return null;
  }

  return (
    <Slider
      value={settings?.typed_input_delay_ms ?? 0}
      onChange={(value) => updateSetting("typed_input_delay_ms", value)}
      min={0}
      max={100}
      step={5}
      label={t("settings.advanced.typedInputDelay.title")}
      description={t("settings.advanced.typedInputDelay.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
      formatValue={(value) => `${value}ms`}
    />
  );
};
