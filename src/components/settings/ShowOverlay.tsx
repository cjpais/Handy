import React from "react";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import type { OverlayPosition } from "@/bindings";

interface ShowOverlayProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

const overlayOptions = [
  { value: "none", label: "None" },
  { value: "bottom", label: "Bottom" },
  { value: "top", label: "Top" },
];

export const ShowOverlay: React.FC<ShowOverlayProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const selectedPosition = (getSetting("overlay_position") ||
      "bottom") as OverlayPosition;

    return (
      <SettingContainer
        title="Overlay Position"
        description="Display visual feedback overlay during recording and transcription. On Linux 'None' is recommended."
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <Dropdown
          options={overlayOptions}
          selectedValue={selectedPosition}
          onSelect={(value) =>
            updateSetting("overlay_position", value as OverlayPosition)
          }
          disabled={isUpdating("overlay_position")}
        />
      </SettingContainer>
    );
  },
);
