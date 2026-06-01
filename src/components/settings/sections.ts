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
  shortcuts: {
    labelKey: "sidebar.shortcuts",
    icon: Keyboard,
    component: ShortcutsSettings,
    enabled: () => true,
  },
  capture: {
    labelKey: "sidebar.capture",
    icon: Mic,
    component: CaptureSettings,
    enabled: () => true,
  },
  transcription: {
    labelKey: "sidebar.transcription",
    icon: AudioLines,
    component: TranscriptionSettings,
    enabled: () => true,
  },
  postprocessing: {
    labelKey: "sidebar.postProcessing",
    icon: Sparkles,
    component: PostProcessingSettings,
    enabled: () => true,
  },
  output: {
    labelKey: "sidebar.output",
    icon: ArrowRight,
    component: OutputSettings,
    enabled: () => true,
  },
  summarisation: {
    labelKey: "sidebar.summarisation",
    icon: FileText,
    component: SummarisationSettings,
    enabled: () => true,
  },
  app: {
    labelKey: "sidebar.app",
    icon: Cog,
    component: AppSettings,
    enabled: () => true,
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
