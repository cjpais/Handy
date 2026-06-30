import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronLeft } from "lucide-react";
import { useSettings } from "../../hooks/useSettings";
import {
  SETTINGS_SECTIONS,
  SETTINGS_GROUP_ORDER,
  SETTINGS_GROUP_LABEL_KEYS,
  type SettingsSection,
  type SettingsGroup,
} from "./sections";

interface SettingsPanelProps {
  onBack: () => void;
}

export const SettingsPanel: React.FC<SettingsPanelProps> = ({ onBack }) => {
  const { t } = useTranslation();
  const { settings } = useSettings();
  const [activeSection, setActiveSection] =
    useState<SettingsSection>("shortcuts");

  const availableSections = Object.entries(SETTINGS_SECTIONS)
    .filter(([_, config]) => config.enabled(settings))
    .map(([id, config]) => ({ id: id as SettingsSection, ...config }));

  // If the active section becomes unavailable (e.g. debug toggled off), fall
  // back to shortcuts so we never render an empty panel.
  const resolvedSection = availableSections.some((s) => s.id === activeSection)
    ? activeSection
    : "shortcuts";
  const ActiveComponent = SETTINGS_SECTIONS[resolvedSection].component;

  const sectionsByGroup = SETTINGS_GROUP_ORDER.reduce<
    Record<SettingsGroup, typeof availableSections>
  >(
    (acc, group) => {
      acc[group] = availableSections.filter((s) => s.group === group);
      return acc;
    },
    { capture: [], dictate: [], keep: [], app: [] },
  );

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center gap-2 px-4 py-3 border-b border-mid-gray/20">
        <button
          onClick={onBack}
          className="flex items-center gap-1 p-1.5 -ms-1.5 rounded-lg text-text/70 hover:text-text hover:bg-mid-gray/20 transition-colors cursor-pointer"
          title={t("settings.back")}
        >
          <ChevronLeft width={18} height={18} className="shrink-0" />
        </button>
        <h1 className="text-sm font-semibold">{t("sidebar.settings")}</h1>
      </div>
      <div className="flex flex-1 overflow-hidden">
        <nav className="flex flex-col w-40 shrink-0 gap-3 p-2 border-e border-mid-gray/20 overflow-y-auto">
          {SETTINGS_GROUP_ORDER.map((group) => {
            const sections = sectionsByGroup[group];
            if (sections.length === 0) return null;
            return (
              <div key={group} className="flex flex-col gap-0.5">
                <p className="px-2 py-1 text-xs font-semibold uppercase tracking-wider text-text/40 select-none">
                  {t(SETTINGS_GROUP_LABEL_KEYS[group])}
                </p>
                {sections.map((section) => {
                  const Icon = section.icon;
                  const isActive = resolvedSection === section.id;
                  return (
                    <div
                      key={section.id}
                      className={`flex gap-2 items-center p-2 w-full rounded-lg cursor-pointer transition-colors ${
                        isActive
                          ? "bg-logo-primary/80"
                          : "hover:bg-mid-gray/20 hover:opacity-100 opacity-85"
                      }`}
                      onClick={() => setActiveSection(section.id)}
                    >
                      <Icon width={20} height={20} className="shrink-0" />
                      <p
                        className="text-sm font-medium truncate"
                        title={t(section.labelKey)}
                      >
                        {t(section.labelKey)}
                      </p>
                    </div>
                  );
                })}
              </div>
            );
          })}
        </nav>
        <div className="flex-1 overflow-y-auto">
          <div className="flex flex-col items-center p-4 gap-4">
            <ActiveComponent />
          </div>
        </div>
      </div>
    </div>
  );
};
