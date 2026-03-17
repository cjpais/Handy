import React from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import type { FloatingButtonPosition as FloatingButtonPositionType } from "@/bindings";

interface FloatingButtonPositionProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const FloatingButtonPosition: React.FC<FloatingButtonPositionProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const positionOptions = [
      {
        value: "bottom_center",
        label: t("settings.advanced.floatingButton.position.options.bottom_center"),
      },
      {
        value: "top_left",
        label: t("settings.advanced.floatingButton.position.options.top_left"),
      },
      {
        value: "top_right",
        label: t("settings.advanced.floatingButton.position.options.top_right"),
      },
      {
        value: "bottom_left",
        label: t(
          "settings.advanced.floatingButton.position.options.bottom_left",
        ),
      },
      {
        value: "bottom_right",
        label: t(
          "settings.advanced.floatingButton.position.options.bottom_right",
        ),
      },
      {
        value: "center_left",
        label: t(
          "settings.advanced.floatingButton.position.options.center_left",
        ),
      },
      {
        value: "center_right",
        label: t(
          "settings.advanced.floatingButton.position.options.center_right",
        ),
      },
    ];

    const selectedPosition = (getSetting("floating_button_position") ||
      "bottom_center") as FloatingButtonPositionType;

    return (
      <SettingContainer
        title={t("settings.advanced.floatingButton.position.title")}
        description={t(
          "settings.advanced.floatingButton.position.description",
        )}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <Dropdown
          options={positionOptions}
          selectedValue={selectedPosition}
          onSelect={(value) =>
            updateSetting(
              "floating_button_position",
              value as FloatingButtonPositionType,
            )
          }
          disabled={isUpdating("floating_button_position")}
        />
      </SettingContainer>
    );
  });
