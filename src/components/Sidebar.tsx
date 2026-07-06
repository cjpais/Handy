import React from "react";
import logo from "../assets/logo.png";
import { useTranslation } from "react-i18next";
import {
  Cog,
  FlaskConical,
  History,
  Info,
  Sparkles,
  Cpu,
  Users,
  Sliders,
} from "lucide-react";
import { useSettings } from "../hooks/useSettings";
import {
  GeneralSettings,
  AdvancedSettings,
  HistorySettings,
  MeetingsSettings,
  DebugSettings,
  AboutSettings,
  PostProcessingSettings,
  ModelsSettings,
} from "./settings";

export type SidebarSection = keyof typeof SECTIONS_CONFIG;

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
  isProdHidden?: boolean;
}

export const SECTIONS_CONFIG = {
  general: {
    labelKey: "sidebar.general",
    icon: Sliders,
    component: GeneralSettings,
    enabled: () => true,
  },
  models: {
    labelKey: "sidebar.models",
    icon: Cpu,
    component: ModelsSettings,
    enabled: () => true,
    isProdHidden: true,
  },
  advanced: {
    labelKey: "sidebar.advanced",
    icon: Cog,
    component: AdvancedSettings,
    enabled: () => true,
  },
  history: {
    labelKey: "sidebar.history",
    icon: History,
    component: HistorySettings,
    enabled: () => true,
    isProdHidden: true,
  },
  meetings: {
    labelKey: "sidebar.meetings",
    icon: Users,
    component: MeetingsSettings,
    enabled: () => true,
    isProdHidden: true,
  },
  postprocessing: {
    labelKey: "sidebar.postProcessing",
    icon: Sparkles,
    component: PostProcessingSettings,
    enabled: () => true,
    isProdHidden: true,
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

interface SidebarProps {
  activeSection: SidebarSection;
  onSectionChange: (section: SidebarSection) => void;
  simulateProd: boolean;
  onToggleSimulateProd: () => void;
}

export const Sidebar: React.FC<SidebarProps> = ({
  activeSection,
  onSectionChange,
  simulateProd,
  onToggleSimulateProd,
}) => {
  const { t } = useTranslation();
  const { settings } = useSettings();

  const isRealProd = !import.meta.env.DEV;
  const isSimulatingOrRealProd = isRealProd || simulateProd;

  const availableSections = Object.entries(SECTIONS_CONFIG)
    .filter(([_, config]) => {
      const sectionConfig = config as SectionConfig;
      if (isSimulatingOrRealProd && sectionConfig.isProdHidden) {
        return false;
      }
      return sectionConfig.enabled(settings);
    })
    .map(([id, config]) => ({ id: id as SidebarSection, ...config }));

  return (
    <div className="flex flex-col w-44 h-full border-e border-stone-mist bg-orange-off-white items-center px-3 py-4 select-none justify-between">
      <div className="flex flex-col w-full items-center flex-1 overflow-hidden">
        <div className="flex items-center justify-center gap-2 mb-6 mt-2 shrink-0">
          <img
            src={logo}
            alt="Logo"
            className="h-7 w-7 object-contain select-none"
          />
          <span className="text-xl font-bold text-charcoal font-cooper tracking-wide">
            Thegai
          </span>
        </div>
        <div className="flex-1 flex flex-col w-full items-center gap-1.5 pt-4 border-t border-stone-mist overflow-y-auto scrollbar-none">
          {availableSections.map((section) => {
            const Icon = section.icon;
            const isActive = activeSection === section.id;

            return (
              <div
                key={section.id}
                className={`flex gap-3 items-center px-3 py-2 w-full rounded-[8px] cursor-pointer transition-all duration-200 ${
                  isActive
                    ? "bg-[#1d7a46] text-[#fffbf7] font-semibold shadow-sm"
                    : "text-bark-grey hover:bg-stone-mist/30 hover:text-charcoal"
                }`}
                onClick={() => onSectionChange(section.id)}
              >
                <Icon width={18} height={18} className="shrink-0 opacity-85" />
                <p
                  className="text-[11px] font-semibold uppercase tracking-[0.04em] font-mono truncate"
                  title={t(section.labelKey)}
                >
                  {t(section.labelKey)}
                </p>
              </div>
            );
          })}
        </div>
      </div>

      {import.meta.env.DEV && (
        <div className="w-full pt-4 border-t border-stone-mist mt-4 shrink-0 px-1">
          <button
            onClick={onToggleSimulateProd}
            className={`w-full flex items-center justify-center gap-2 py-1.5 px-2 rounded-md border text-[10px] font-mono font-bold tracking-wide uppercase transition-all duration-200 ${
              simulateProd
                ? "bg-amber-500/10 border-amber-500/30 text-amber-600 hover:bg-amber-500/20"
                : "bg-[#1d7a46]/5 border-[#1d7a46]/20 text-[#1d7a46] hover:bg-[#1d7a46]/10"
            }`}
            title="Toggle between Developer and simulated Production view of settings"
          >
            <span
              className={`w-1.5 h-1.5 rounded-full ${simulateProd ? "bg-amber-500 animate-pulse" : "bg-[#1d7a46]"}`}
            />
            {simulateProd ? "SIMULATED PROD" : "DEV VIEW"}
          </button>
        </div>
      )}
    </div>
  );
};
