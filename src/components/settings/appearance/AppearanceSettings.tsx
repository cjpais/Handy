import React from "react";
import { useTranslation } from "react-i18next";
import { Check } from "lucide-react";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";
import { useSettings } from "../../../hooks/useSettings";
import {
  AccentTheme,
  THEME_OPTIONS,
  THEME_COLORS,
  applyTheme,
} from "../../../theme";

export const AppearanceSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();

  const selectedTheme = (getSetting("accent_theme") as AccentTheme) ?? "pink";
  const updating = isUpdating("accent_theme");

  const handleThemeChange = async (themeId: AccentTheme) => {
    if (updating || themeId === selectedTheme) return;

    // Apply theme immediately for instant feedback
    applyTheme(themeId);

    // Persist to settings
    await updateSetting("accent_theme", themeId);
  };

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("settings.appearance.title")}>
        <SettingContainer
          title={t("settings.appearance.accentColor.title")}
          description={t("settings.appearance.accentColor.description")}
          descriptionMode="tooltip"
          grouped={true}
        >
          <div className="flex gap-2 flex-wrap">
            {THEME_OPTIONS.map((theme) => {
              const colors = THEME_COLORS[theme.id];
              const isSelected = selectedTheme === theme.id;

              return (
                <button
                  key={theme.id}
                  onClick={() => handleThemeChange(theme.id)}
                  disabled={updating}
                  className={`relative w-8 h-8 rounded-full border-2 transition-all duration-200 hover:scale-110 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-logo-primary disabled:opacity-50 disabled:cursor-not-allowed ${
                    isSelected
                      ? "border-text ring-2 ring-offset-1 ring-logo-primary"
                      : "border-mid-gray/40 hover:border-mid-gray"
                  }`}
                  style={{ backgroundColor: colors.primary }}
                  title={t(theme.nameKey)}
                  aria-label={t("settings.appearance.accentColor.selectTheme", {
                    theme: t(theme.nameKey),
                  })}
                  aria-pressed={isSelected}
                >
                  {isSelected && (
                    <Check
                      className="absolute inset-0 m-auto text-white drop-shadow-md"
                      size={16}
                      strokeWidth={3}
                    />
                  )}
                </button>
              );
            })}
          </div>
        </SettingContainer>
      </SettingsGroup>
    </div>
  );
};
