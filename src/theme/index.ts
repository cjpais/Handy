import { commands } from "@/bindings";

// Accent color themes
export type AccentTheme =
  | "pink"
  | "blue"
  | "green"
  | "purple"
  | "orange"
  | "teal";

export const THEME_COLORS: Record<
  AccentTheme,
  {
    primary: string;
    background: string;
    light: string;
    stroke: string;
  }
> = {
  pink: {
    primary: "#faa2ca",
    background: "#da5893",
    light: "#ffe5ee",
    stroke: "#382731",
  },
  blue: {
    primary: "#7fb8f0",
    background: "#3a7bd5",
    light: "#e0f0ff",
    stroke: "#1e3a5f",
  },
  green: {
    primary: "#7fd4a0",
    background: "#3da36d",
    light: "#e0f5eb",
    stroke: "#1e4a30",
  },
  purple: {
    primary: "#b89af0",
    background: "#7b5ac5",
    light: "#f0e5ff",
    stroke: "#3a2860",
  },
  orange: {
    primary: "#f0b87f",
    background: "#d58a3a",
    light: "#fff0e0",
    stroke: "#5f3a1e",
  },
  teal: {
    primary: "#7fd4d4",
    background: "#3da3a3",
    light: "#e0f5f5",
    stroke: "#1e4a4a",
  },
};

export const THEME_OPTIONS = [
  { id: "pink" as const, nameKey: "settings.appearance.themes.pink" },
  { id: "blue" as const, nameKey: "settings.appearance.themes.blue" },
  { id: "green" as const, nameKey: "settings.appearance.themes.green" },
  { id: "purple" as const, nameKey: "settings.appearance.themes.purple" },
  { id: "orange" as const, nameKey: "settings.appearance.themes.orange" },
  { id: "teal" as const, nameKey: "settings.appearance.themes.teal" },
];

// Overlay visual themes
export type OverlayTheme = "pill" | "minimal" | "glassmorphism";

export const OVERLAY_THEME_OPTIONS = [
  {
    id: "pill" as const,
    nameKey: "settings.appearance.overlayThemes.pill.name",
    descriptionKey: "settings.appearance.overlayThemes.pill.description",
  },
  {
    id: "minimal" as const,
    nameKey: "settings.appearance.overlayThemes.minimal.name",
    descriptionKey: "settings.appearance.overlayThemes.minimal.description",
  },
  {
    id: "glassmorphism" as const,
    nameKey: "settings.appearance.overlayThemes.glassmorphism.name",
    descriptionKey:
      "settings.appearance.overlayThemes.glassmorphism.description",
  },
];

export const applyTheme = (theme: AccentTheme): void => {
  document.documentElement.setAttribute("data-theme", theme);
};

export const syncThemeFromSettings = async (): Promise<AccentTheme> => {
  try {
    const result = await commands.getAppSettings();
    if (result.status === "ok") {
      const theme = (result.data.accent_theme as AccentTheme) || "pink";
      applyTheme(theme);
      return theme;
    }
  } catch (e) {
    console.warn("Failed to sync theme from settings:", e);
  }
  return "pink";
};

export const syncOverlayThemeFromSettings = async (): Promise<OverlayTheme> => {
  try {
    const result = await commands.getAppSettings();
    if (result.status === "ok") {
      return (result.data.overlay_theme as OverlayTheme) || "pill";
    }
  } catch (e) {
    console.warn("Failed to sync overlay theme from settings:", e);
  }
  return "pill";
};

export const getThemeColors = (
  theme: AccentTheme
): (typeof THEME_COLORS)[AccentTheme] => THEME_COLORS[theme];
