import React from "react";
import { useTranslation } from "react-i18next";
import type { ChineseConversion } from "@/bindings";
import { useSettings } from "../../hooks/useSettings";
import { Dropdown, type DropdownOption } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";

interface ChineseConversionProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const ChineseConversionSetting: React.FC<ChineseConversionProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const selectedConversion = (getSetting("chinese_conversion") ||
      "auto") as ChineseConversion;
    const options: DropdownOption[] = [
      {
        value: "auto",
        label: t("settings.advanced.chineseConversion.options.auto"),
      },
      {
        value: "off",
        label: t("settings.advanced.chineseConversion.options.off"),
      },
      {
        value: "traditional_taiwan",
        label: t(
          "settings.advanced.chineseConversion.options.traditionalTaiwan",
        ),
      },
      {
        value: "traditional_hong_kong",
        label: t(
          "settings.advanced.chineseConversion.options.traditionalHongKong",
        ),
      },
      {
        value: "simplified",
        label: t("settings.advanced.chineseConversion.options.simplified"),
      },
    ];

    return (
      <SettingContainer
        title={t("settings.advanced.chineseConversion.title")}
        description={t("settings.advanced.chineseConversion.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <Dropdown
          options={options}
          selectedValue={selectedConversion}
          onSelect={(value) =>
            updateSetting("chinese_conversion", value as ChineseConversion)
          }
          disabled={isUpdating("chinese_conversion")}
        />
      </SettingContainer>
    );
  });
