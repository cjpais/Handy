import React from "react";
import { useTranslation } from "react-i18next";
import { Check, FileText, Mic, X } from "lucide-react";
import type { OverlayTheme as OverlayThemeValue } from "@/bindings";
import { useSettings } from "../../hooks/useSettings";
import { SettingContainer } from "../ui/SettingContainer";

interface OverlayThemeProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

interface ThemeView {
  value: OverlayThemeValue;
  name: string;
  panelClass: string;
  overlayClass: string;
  micClass: string;
  barClass: string;
  finishClass: string;
  cancelClass: string;
  textClass: string;
}

const bars = [6, 11, 16, 20, 13, 18, 9];

const MiniBars: React.FC<{ barClass: string }> = ({ barClass }) => (
  <div className="flex h-5 items-end justify-center gap-1 overflow-hidden">
    {bars.map((height, index) => (
      <span
        key={index}
        className={`w-1.5 rounded-sm ${barClass}`}
        style={{ height }}
      />
    ))}
  </div>
);

const ThemeCard: React.FC<{
  view: ThemeView;
  selected: boolean;
  disabled: boolean;
  transcribingText: string;
  recordingText: string;
  onSelect: () => void;
}> = ({
  view,
  selected,
  disabled,
  transcribingText,
  recordingText,
  onSelect,
}) => (
  <button
    type="button"
    aria-pressed={selected}
    disabled={disabled}
    onClick={onSelect}
    className={`w-full rounded-lg border p-3 text-start transition-all ${
      selected
        ? "border-[#3e7288] bg-[#e8f1f4] shadow-sm"
        : "border-mid-gray/25 bg-mid-gray/5 hover:border-[#3e7288]/70 hover:bg-[#e8f1f4]/55"
    } ${disabled ? "cursor-not-allowed opacity-60" : "cursor-pointer"}`}
  >
    <div className="flex items-center justify-between gap-3">
      <span className="text-sm font-semibold text-text">{view.name}</span>
      <span
        className={`flex h-4 w-4 items-center justify-center rounded-full border ${
          selected
            ? "border-[#3e7288] bg-[#3e7288] text-white"
            : "border-mid-gray/40"
        }`}
      >
        {selected && <Check size={12} strokeWidth={2.6} />}
      </span>
    </div>

    <div className={`mt-3 rounded-md p-2 ${view.panelClass}`}>
      <div className="mb-1 text-[11px] font-medium text-text/60">
        {recordingText}
      </div>
      <div
        className={`grid h-8 grid-cols-[22px_minmax(62px,1fr)_auto] items-center gap-1 rounded-full px-2 shadow-sm ${view.overlayClass}`}
      >
        <Mic size={15} strokeWidth={2.2} className={view.micClass} />
        <MiniBars barClass={view.barClass} />
        <div className="flex items-center">
          <Check size={14} strokeWidth={2.4} className={view.finishClass} />
          <X size={14} strokeWidth={2.4} className={view.cancelClass} />
        </div>
      </div>

      <div className="mb-1 mt-3 text-[11px] font-medium text-text/60">
        {transcribingText}
      </div>
      <div
        className={`grid h-8 grid-cols-[22px_minmax(62px,1fr)_auto] items-center gap-1 rounded-full px-2 shadow-sm ${view.overlayClass}`}
      >
        <FileText size={15} strokeWidth={2.2} className={view.micClass} />
        <span
          className={`truncate text-center text-[11px] font-semibold ${view.textClass}`}
        >
          {transcribingText}
        </span>
        <span className="h-4 w-7" />
      </div>
    </div>
  </button>
);

export const OverlayTheme: React.FC<OverlayThemeProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const selectedTheme = (getSetting("overlay_theme") ||
      "calm") as OverlayThemeValue;

    const views: ThemeView[] = [
      {
        value: "classic",
        name: t("settings.advanced.overlayTheme.options.classic"),
        panelClass: "bg-neutral-200/60",
        overlayClass: "border border-[#3a363f] bg-[#242428]",
        micClass: "text-[#faa2ca]",
        barClass: "bg-[#ffe5ee]",
        finishClass: "text-[#faa2ca]",
        cancelClass: "text-[#faa2ca]",
        textClass: "text-white",
      },
      {
        value: "calm",
        name: t("settings.advanced.overlayTheme.options.calm"),
        panelClass: "bg-slate-100/70",
        overlayClass: "border border-[#c8d2da] bg-[#f7fafc]",
        micClass: "text-[#2f5f73]",
        barClass: "bg-[#3e7288]",
        finishClass: "text-[#2f6f57]",
        cancelClass: "text-[#587080]",
        textClass: "text-[#243743]",
      },
      {
        value: "dark",
        name: t("settings.advanced.overlayTheme.options.dark"),
        panelClass: "bg-slate-200/70",
        overlayClass: "border border-[#3a4a55] bg-[#1f2933]",
        micClass: "text-[#8bb9c9]",
        barClass: "bg-[#8bb9c9]",
        finishClass: "text-[#95d1bb]",
        cancelClass: "text-[#b8c7d0]",
        textClass: "text-[#edf4f7]",
      },
    ];
    const isThemeUpdating = isUpdating("overlay_theme");

    return (
      <SettingContainer
        title={t("settings.advanced.overlayTheme.title")}
        description={t("settings.advanced.overlayTheme.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="grid w-full grid-cols-1 gap-3 md:grid-cols-3">
          {views.map((view) => (
            <ThemeCard
              key={view.value}
              view={view}
              selected={selectedTheme === view.value}
              disabled={isThemeUpdating}
              recordingText={t(
                "settings.advanced.overlayTheme.states.recording",
              )}
              transcribingText={t(
                "settings.advanced.overlayTheme.states.transcribing",
              )}
              onSelect={() => updateSetting("overlay_theme", view.value)}
            />
          ))}
        </div>
      </SettingContainer>
    );
  },
);
