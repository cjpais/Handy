import React from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "@/hooks/useSettings";
import { applyTheme, THEME_OPTIONS } from "@/lib/utils/theme";
import type { Theme } from "@/bindings";

interface ThemeSelectorProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const ThemeSelector: React.FC<ThemeSelectorProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { settings, updateSetting } = useSettings();

    const currentTheme: Theme = settings?.theme ?? "system";

    const themeOptions = THEME_OPTIONS.map((value) => ({
      value,
      label: t(`theme.options.${value}`),
    }));

    const handleThemeChange = (value: string) => {
      const theme = value as Theme;
      applyTheme(theme);
      updateSetting("theme", theme);
    };

    return (
      <SettingContainer
        title={t("theme.title")}
        description={t("theme.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <Dropdown
          options={themeOptions}
          selectedValue={currentTheme}
          onSelect={handleThemeChange}
        />
      </SettingContainer>
    );
  },
);

ThemeSelector.displayName = "ThemeSelector";
