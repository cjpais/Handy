import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../../ui/ToggleSwitch";
import { applyStyle, getStoredStyle } from "@/lib/utils/style";

interface StylePreviewToggleProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/** Switches the whole app between today's look and the v2 style (temporary). */
export const StylePreviewToggle: React.FC<StylePreviewToggleProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const [isV2, setIsV2] = useState(() => getStoredStyle() === "v2");

  return (
    <ToggleSwitch
      checked={isV2}
      onChange={(enabled) => {
        applyStyle(enabled ? "v2" : "current");
        setIsV2(enabled);
      }}
      label={t("settings.debug.stylePreview.title")}
      description={t("settings.debug.stylePreview.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
    />
  );
};
