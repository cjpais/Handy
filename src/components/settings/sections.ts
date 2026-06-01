import React from "react";
import { Cog, FlaskConical, Info, Sparkles, FileText, Cpu } from "lucide-react";
import GoldfishIcon from "../icons/HandyHand";
import {
  GeneralSettings,
  AdvancedSettings,
  DebugSettings,
  AboutSettings,
  PostProcessingSettings,
  SummarisationSettings,
  ModelsSettings,
} from "./index";

interface IconProps {
  width?: number | string;
  height?: number | string;
  size?: number | string;
  className?: string;
  [key: string]: any;
}

interface SectionConfig {
  labelKey: string;
  icon: React.ComponentType<IconProps>;
  component: React.ComponentType;
  enabled: (settings: any) => boolean;
}

export const SETTINGS_SECTIONS = {
  general: {
    labelKey: "sidebar.general",
    icon: GoldfishIcon,
    component: GeneralSettings,
    enabled: () => true,
  },
  models: {
    labelKey: "sidebar.models",
    icon: Cpu,
    component: ModelsSettings,
    enabled: () => true,
  },
  advanced: {
    labelKey: "sidebar.advanced",
    icon: Cog,
    component: AdvancedSettings,
    enabled: () => true,
  },
  postprocessing: {
    labelKey: "sidebar.postProcessing",
    icon: Sparkles,
    component: PostProcessingSettings,
    enabled: (settings) => settings?.post_process_enabled ?? false,
  },
  summarisation: {
    labelKey: "sidebar.summarisation",
    icon: FileText,
    component: SummarisationSettings,
    enabled: (settings) => settings?.summarize_enabled ?? false,
  },
  debug: {
    labelKey: "sidebar.debug",
    icon: FlaskConical,
    component: DebugSettings,
    enabled: (settings) => settings?.debug_mode ?? false,
  },
  about: {
    labelKey: "sidebar.about",
    icon: Info,
    component: AboutSettings,
    enabled: () => true,
  },
} as const satisfies Record<string, SectionConfig>;

export type SettingsSection = keyof typeof SETTINGS_SECTIONS;
