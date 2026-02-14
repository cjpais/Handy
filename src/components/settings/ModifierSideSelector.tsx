import React from "react";
import { useTranslation } from "react-i18next";
import { SettingContainer } from "../ui/SettingContainer";
import { SegmentedControl } from "../ui/SegmentedControl";
import { useSettings } from "../../hooks/useSettings";
import { commands } from "@/bindings";
import { toast } from "sonner";

const MODIFIER_SIDE_OPTIONS = [
  { value: "any", label: "Any" },
  { value: "left", label: "Left" },
  { value: "right", label: "Right" },
];

interface ModifierSideSelectorProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const ModifierSideSelector: React.FC<ModifierSideSelectorProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, refreshSettings } = useSettings();
  const currentSide = getSetting("modifier_side") ?? "any";

  const handleSelect = async (value: string) => {
    if (value === currentSide) return;

    try {
      const result = await commands.changeModifierSideSetting(value);

      if (result.status === "error") {
        console.error("Failed to update modifier side:", result.error);
        toast.error(String(result.error));
        return;
      }

      await refreshSettings();
    } catch (error) {
      console.error("Failed to update modifier side:", error);
      toast.error(String(error));
    }
  };

  return (
    <SettingContainer
      title={t("settings.modifierSide.title")}
      description={t("settings.modifierSide.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
      layout="horizontal"
    >
      <SegmentedControl
        options={MODIFIER_SIDE_OPTIONS}
        value={currentSide}
        onChange={handleSelect}
      />
    </SettingContainer>
  );
};
