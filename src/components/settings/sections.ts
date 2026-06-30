import React from "react";
import {
  Cog,
  FlaskConical,
  Info,
  Sparkles,
  FileText,
  Mic,
  ArrowRight,
  Keyboard,
  AudioLines,
  Clock,
} from "lucide-react";
import {
  ShortcutsSettings,
  CaptureSettings,
  TranscriptionSettings,
  PostProcessingSettings,
  OutputSettings,
  SummarisationSettings,
  AppSettings,
  DebugSettings,
  AboutSettings,
  RetentionSettings,
} from "./index";

interface IconProps {
  width?: number | string;
  height?: number | string;
  size?: number | string;
  className?: string;
  [key: string]: any;
}

export type SettingsGroup = "capture" | "dictate" | "keep" | "app";

interface SectionConfig {
  labelKey: string;
  icon: React.ComponentType<IconProps>;
  component: React.ComponentType;
  group: SettingsGroup;
  enabled: (settings: any) => boolean;
}

export const SETTINGS_SECTIONS = {
  shortcuts: {
    labelKey: "sidebar.shortcuts",
    icon: Keyboard,
    component: ShortcutsSettings,
    group: "capture" as SettingsGroup,
    enabled: () => true,
  },
  capture: {
    labelKey: "sidebar.capture",
    icon: Mic,
    component: CaptureSettings,
    group: "capture" as SettingsGroup,
    enabled: () => true,
  },
  transcription: {
    labelKey: "sidebar.transcription",
    icon: AudioLines,
    component: TranscriptionSettings,
    group: "capture" as SettingsGroup,
    enabled: () => true,
  },
  postprocessing: {
    labelKey: "sidebar.clean",
    icon: Sparkles,
    component: PostProcessingSettings,
    group: "capture" as SettingsGroup,
    enabled: () => true,
  },
  output: {
    labelKey: "sidebar.output",
    icon: ArrowRight,
    component: OutputSettings,
    group: "dictate" as SettingsGroup,
    enabled: () => true,
  },
  summarisation: {
    labelKey: "sidebar.summarisation",
    icon: FileText,
    component: SummarisationSettings,
    group: "keep" as SettingsGroup,
    enabled: () => true,
  },
  retention: {
    labelKey: "sidebar.retention",
    icon: Clock,
    component: RetentionSettings,
    group: "keep" as SettingsGroup,
    enabled: () => true,
  },
  app: {
    labelKey: "sidebar.app",
    icon: Cog,
    component: AppSettings,
    group: "app" as SettingsGroup,
    enabled: () => true,
  },
  debug: {
    labelKey: "sidebar.debug",
    icon: FlaskConical,
    component: DebugSettings,
    group: "app" as SettingsGroup,
    enabled: (settings) => settings?.debug_mode ?? false,
  },
  about: {
    labelKey: "sidebar.about",
    icon: Info,
    component: AboutSettings,
    group: "app" as SettingsGroup,
    enabled: () => true,
  },
} as const satisfies Record<string, SectionConfig>;

export type SettingsSection = keyof typeof SETTINGS_SECTIONS;

export const SETTINGS_GROUP_ORDER: SettingsGroup[] = [
  "capture",
  "dictate",
  "keep",
  "app",
];

export const SETTINGS_GROUP_LABEL_KEYS: Record<SettingsGroup, string> = {
  capture: "sidebar.groups.capture",
  dictate: "sidebar.groups.dictate",
  keep: "sidebar.groups.keep",
  app: "sidebar.groups.app",
};
