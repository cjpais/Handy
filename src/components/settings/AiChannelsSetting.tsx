import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronUp, ChevronDown, Eye, EyeOff } from "lucide-react";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import type { AiChannelConfig } from "@/bindings";
import { DEFAULT_AI_CHANNELS } from "@/config/aiChannels";

interface AiChannelsSettingProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

function buildEffectiveList(stored: AiChannelConfig[] | null | undefined): AiChannelConfig[] {
  if (!stored || stored.length === 0) {
    return DEFAULT_AI_CHANNELS.map((c) => ({ id: c.id, visible: true }));
  }
  const storedIds = new Set(stored.map((c) => c.id));
  const extra = DEFAULT_AI_CHANNELS.filter((c) => !storedIds.has(c.id)).map(
    (c) => ({ id: c.id, visible: true }),
  );
  return [...stored, ...extra];
}

export const AiChannelsSetting: React.FC<AiChannelsSettingProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting } = useSettings();

  const stored = getSetting("ai_channels") ?? null;
  const [list, setList] = useState<AiChannelConfig[]>(() =>
    buildEffectiveList(stored),
  );

  useEffect(() => {
    setList(buildEffectiveList(stored));
  }, [JSON.stringify(stored)]);

  const save = (next: AiChannelConfig[]) => {
    setList(next);
    void updateSetting("ai_channels", next);
  };

  const toggle = (id: string) => {
    save(list.map((c) => (c.id === id ? { ...c, visible: !c.visible } : c)));
  };

  const move = (index: number, dir: -1 | 1) => {
    const next = [...list];
    const target = index + dir;
    if (target < 0 || target >= next.length) return;
    [next[index], next[target]] = [next[target], next[index]];
    save(next);
  };

  return (
    <SettingContainer
      title={t("settings.advanced.aiChannels.title", {
        defaultValue: "AI 频道管理",
      })}
      description={t("settings.advanced.aiChannels.description", {
        defaultValue: "选择在主页显示哪些 AI 平台，并调整排列顺序。",
      })}
      descriptionMode={descriptionMode}
      grouped={grouped}
      layout="stacked"
    >
      <div className="flex flex-col gap-1 rounded-xl border border-slate-200/80 bg-white/60 p-1">
        {list.map((cfg, i) => {
          const meta = DEFAULT_AI_CHANNELS.find((c) => c.id === cfg.id);
          if (!meta) return null;
          return (
            <div
              key={cfg.id}
              className={`flex items-center gap-3 rounded-lg px-3 py-2 transition ${
                cfg.visible ? "bg-white shadow-sm" : "opacity-50"
              }`}
            >
              <div
                className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-lg text-xs font-bold text-white ${meta.iconClassName} ${meta.iconTextClassName ?? ""}`}
              >
                {meta.mark}
              </div>
              <div className="min-w-0 flex-1">
                <p className="truncate text-sm font-medium text-slate-800">
                  {meta.title}
                </p>
                <p className="truncate text-xs text-slate-400">
                  {meta.subtitle}
                </p>
              </div>
              <div className="flex shrink-0 items-center gap-1">
                <button
                  type="button"
                  onClick={() => toggle(cfg.id)}
                  className="rounded-md p-1 text-slate-400 transition hover:bg-slate-100 hover:text-slate-700"
                  title={
                    cfg.visible
                      ? t("settings.advanced.aiChannels.hide", {
                          defaultValue: "隐藏",
                        })
                      : t("settings.advanced.aiChannels.show", {
                          defaultValue: "显示",
                        })
                  }
                >
                  {cfg.visible ? (
                    <Eye className="h-4 w-4" />
                  ) : (
                    <EyeOff className="h-4 w-4" />
                  )}
                </button>
                <button
                  type="button"
                  onClick={() => move(i, -1)}
                  disabled={i === 0}
                  className="rounded-md p-1 text-slate-400 transition hover:bg-slate-100 hover:text-slate-700 disabled:opacity-30"
                >
                  <ChevronUp className="h-4 w-4" />
                </button>
                <button
                  type="button"
                  onClick={() => move(i, 1)}
                  disabled={i === list.length - 1}
                  className="rounded-md p-1 text-slate-400 transition hover:bg-slate-100 hover:text-slate-700 disabled:opacity-30"
                >
                  <ChevronDown className="h-4 w-4" />
                </button>
              </div>
            </div>
          );
        })}
      </div>
    </SettingContainer>
  );
};
