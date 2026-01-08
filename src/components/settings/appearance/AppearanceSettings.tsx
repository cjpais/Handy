import React from "react";
import { useTranslation } from "react-i18next";
import { Check } from "lucide-react";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";
import { ToggleSwitch } from "../../ui/ToggleSwitch";
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

// Preset bar colors
const BAR_COLOR_OPTIONS = [
  { id: "accent", color: null, nameKey: "settings.appearance.barColor.accent" },
  { id: "#ffffff", color: "#ffffff", nameKey: "settings.appearance.barColor.white" },
  { id: "#ff6b6b", color: "#ff6b6b", nameKey: "settings.appearance.barColor.red" },
  { id: "#4ecdc4", color: "#4ecdc4", nameKey: "settings.appearance.barColor.cyan" },
  { id: "#ffe66d", color: "#ffe66d", nameKey: "settings.appearance.barColor.yellow" },
  { id: "#95e1d3", color: "#95e1d3", nameKey: "settings.appearance.barColor.mint" },
];

export const AppearanceSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();

  const selectedAccentTheme =
    (getSetting("accent_theme") as AccentTheme) ?? "pink";
  const selectedOverlayTheme =
    (getSetting("overlay_theme") as OverlayTheme) ?? "pill";
  const showIcons = (getSetting("overlay_show_icons") as boolean) ?? true;
  const barsCentered = (getSetting("overlay_bars_centered") as boolean) ?? false;
  const barCount = (getSetting("overlay_bar_count") as number) ?? 9;
  const barSize = (getSetting("overlay_bar_size") as number) ?? 6;
  const barColor = (getSetting("overlay_bar_color") as string) ?? "accent";

  const updatingAccent = isUpdating("accent_theme");
  const updatingOverlay = isUpdating("overlay_theme");
  const updatingShowIcons = isUpdating("overlay_show_icons");
  const updatingBarsCentered = isUpdating("overlay_bars_centered");
  const updatingBarCount = isUpdating("overlay_bar_count");
  const updatingBarSize = isUpdating("overlay_bar_size");
  const updatingBarColor = isUpdating("overlay_bar_color");

  const handleAccentThemeChange = async (themeId: AccentTheme) => {
    if (updatingAccent || themeId === selectedAccentTheme) return;
    applyTheme(themeId);
    await updateSetting("accent_theme", themeId);
  };

  const handleOverlayThemeChange = async (themeId: OverlayTheme) => {
    if (updatingOverlay || themeId === selectedOverlayTheme) return;
    await updateSetting("overlay_theme", themeId);
  };

  const handleShowIconsChange = async (enabled: boolean) => {
    if (updatingShowIcons) return;
    await updateSetting("overlay_show_icons", enabled);
  };

  const handleBarsCenteredChange = async (enabled: boolean) => {
    if (updatingBarsCentered) return;
    await updateSetting("overlay_bars_centered", enabled);
  };

  const handleBarCountChange = async (count: number) => {
    if (updatingBarCount) return;
    await updateSetting("overlay_bar_count", count);
  };

  const handleBarSizeChange = async (size: number) => {
    if (updatingBarSize) return;
    await updateSetting("overlay_bar_size", size);
  };

  const handleBarColorChange = async (color: string) => {
    if (updatingBarColor || color === barColor) return;
    await updateSetting("overlay_bar_color", color);
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
              showIcons={showIcons}
              animate={true}
              barsCentered={barsCentered}
              barCount={barCount}
              barSize={barSize}
              barColor={barColor}
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
                    showIcons={showIcons}
                    animate={false}
                    barsCentered={barsCentered}
                    barColor={barColor}
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

        {/* Show Icons Toggle */}
        <ToggleSwitch
          label={t("settings.appearance.showIcons.label")}
          description={t("settings.appearance.showIcons.description")}
          grouped={true}
          checked={showIcons}
          onChange={handleShowIconsChange}
          disabled={updatingShowIcons}
        />

        {/* Centered Bars Toggle */}
        <ToggleSwitch
          label={t("settings.appearance.barsCentered.label")}
          description={t("settings.appearance.barsCentered.description")}
          grouped={true}
          checked={barsCentered}
          onChange={handleBarsCenteredChange}
          disabled={updatingBarsCentered}
        />

        {/* Bar Count Slider */}
        <SettingContainer
          title={t("settings.appearance.barCount.label")}
          description={t("settings.appearance.barCount.description")}
          grouped={true}
        >
          <div className="flex items-center gap-3">
            <input
              type="range"
              min={3}
              max={15}
              value={barCount}
              onChange={(e) => handleBarCountChange(parseInt(e.target.value))}
              disabled={updatingBarCount}
              className="w-24 accent-logo-primary"
            />
            <span className="text-sm text-mid-gray w-6 text-center">{barCount}</span>
          </div>
        </SettingContainer>

        {/* Bar Size Slider */}
        <SettingContainer
          title={t("settings.appearance.barSize.label")}
          description={t("settings.appearance.barSize.description")}
          grouped={true}
        >
          <div className="flex items-center gap-3">
            <input
              type="range"
              min={2}
              max={12}
              value={barSize}
              onChange={(e) => handleBarSizeChange(parseInt(e.target.value))}
              disabled={updatingBarSize}
              className="w-24 accent-logo-primary"
            />
            <span className="text-sm text-mid-gray w-6 text-center">{barSize}</span>
          </div>
        </SettingContainer>

        {/* Bar Color Picker */}
        <SettingContainer
          title={t("settings.appearance.barColor.label")}
          description={t("settings.appearance.barColor.description")}
          grouped={true}
        >
          <div className="flex gap-2 flex-wrap">
            {BAR_COLOR_OPTIONS.map((option) => {
              const isSelected = barColor === option.id;
              const displayColor = option.color ?? THEME_COLORS[selectedAccentTheme].light;

              return (
                <button
                  key={option.id}
                  onClick={() => handleBarColorChange(option.id)}
                  disabled={updatingBarColor}
                  className={`relative w-7 h-7 rounded-full border-2 transition-all duration-200 hover:scale-110 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-logo-primary disabled:opacity-50 disabled:cursor-not-allowed ${
                    isSelected
                      ? "border-text ring-2 ring-offset-1 ring-logo-primary"
                      : "border-mid-gray/40 hover:border-mid-gray"
                  }`}
                  style={{ backgroundColor: displayColor }}
                  title={t(option.nameKey)}
                  aria-pressed={isSelected}
                >
                  {isSelected && (
                    <Check
                      className="absolute inset-0 m-auto drop-shadow-md"
                      size={14}
                      strokeWidth={3}
                      style={{ color: option.id === "#ffffff" || option.id === "#ffe66d" ? "#333" : "#fff" }}
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
