import React from "react";
import { useTranslation } from "react-i18next";
import { Check } from "lucide-react";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";
import { useSettings } from "../../../hooks/useSettings";
import {
  AccentTheme,
  OverlayTheme,
  THEME_OPTIONS,
  THEME_COLORS,
  OVERLAY_THEME_OPTIONS,
  applyTheme,
} from "../../../theme";
import { OverlayPreview } from "./OverlayPreview";

export const AppearanceSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();

  const selectedAccentTheme =
    (getSetting("accent_theme") as AccentTheme) ?? "pink";
  const selectedOverlayTheme =
    (getSetting("overlay_theme") as OverlayTheme) ?? "pill";
  const updatingAccent = isUpdating("accent_theme");
  const updatingOverlay = isUpdating("overlay_theme");

  const handleAccentThemeChange = async (themeId: AccentTheme) => {
    if (updatingAccent || themeId === selectedAccentTheme) return;

    // Apply theme immediately for instant feedback
    applyTheme(themeId);

    // Persist to settings
    await updateSetting("accent_theme", themeId);
  };

  const handleOverlayThemeChange = async (themeId: OverlayTheme) => {
    if (updatingOverlay || themeId === selectedOverlayTheme) return;

    // Persist to settings
    await updateSetting("overlay_theme", themeId);
  };

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("settings.appearance.title")}>
        {/* Accent Color Picker */}
        <SettingContainer
          title={t("settings.appearance.accentColor.title")}
          description={t("settings.appearance.accentColor.description")}
          descriptionMode="tooltip"
          grouped={true}
        >
          <div className="flex gap-2 flex-wrap">
            {THEME_OPTIONS.map((theme) => {
              const colors = THEME_COLORS[theme.id];
              const isSelected = selectedAccentTheme === theme.id;

              return (
                <button
                  key={theme.id}
                  onClick={() => handleAccentThemeChange(theme.id)}
                  disabled={updatingAccent}
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

      <SettingsGroup title={t("settings.appearance.overlayTheme.title")}>
        {/* Overlay Preview */}
        <div className="px-4 py-3">
          <div className="flex items-center justify-center p-6 bg-mid-gray/10 rounded-lg">
            <OverlayPreview
              accentTheme={selectedAccentTheme}
              overlayTheme={selectedOverlayTheme}
              animate={true}
            />
          </div>
        </div>

        {/* Overlay Theme Selector */}
        <div className="px-4 pb-2">
          <div className="flex flex-col gap-2">
            {OVERLAY_THEME_OPTIONS.map((theme) => {
              const isSelected = selectedOverlayTheme === theme.id;

              return (
                <button
                  key={theme.id}
                  onClick={() => handleOverlayThemeChange(theme.id)}
                  disabled={updatingOverlay}
                  className={`relative flex items-center gap-3 p-3 rounded-lg border transition-all duration-200 text-left ${
                    isSelected
                      ? "border-logo-primary bg-logo-primary/10"
                      : "border-mid-gray/30 hover:border-mid-gray/50 hover:bg-mid-gray/5"
                  } disabled:opacity-50 disabled:cursor-not-allowed`}
                >
                  {/* Mini preview */}
                  <OverlayPreview
                    accentTheme={selectedAccentTheme}
                    overlayTheme={theme.id}
                    animate={false}
                  />

                  {/* Theme info */}
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="font-medium text-sm">
                        {t(theme.nameKey)}
                      </span>
                      {isSelected && (
                        <Check
                          size={16}
                          className="text-logo-primary"
                          strokeWidth={2.5}
                        />
                      )}
                    </div>
                    <p className="text-xs text-mid-gray mt-0.5 truncate">
                      {t(theme.descriptionKey)}
                    </p>
                  </div>
                </button>
              );
            })}
          </div>
        </div>
      </SettingsGroup>
    </div>
  );
};
