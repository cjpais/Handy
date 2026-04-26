import { Globe2, RefreshCw, Settings2 } from "lucide-react";
import { useDeferredValue, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { DEFAULT_AI_CHANNELS } from "@/config/aiChannels";
import { useSettings } from "@/hooks/useSettings";

interface DoubaoHomeProps {
  onOpenSettings: () => void;
}

const DoubaoHome: React.FC<DoubaoHomeProps> = ({ onOpenSettings }) => {
  const { t } = useTranslation();
  const { getSetting } = useSettings();

  const storedChannels = getSetting("ai_channels") ?? null;

  const visibleChannels = useMemo(() => {
    if (!storedChannels || storedChannels.length === 0) {
      return DEFAULT_AI_CHANNELS;
    }
    const storedIds = new Set(storedChannels.map((c) => c.id));
    const extraConfigs = DEFAULT_AI_CHANNELS.filter(
      (c) => !storedIds.has(c.id),
    ).map((c) => ({ id: c.id, visible: true }));
    return [...storedChannels, ...extraConfigs]
      .filter((c) => c.visible !== false)
      .map((c) => {
        const meta = DEFAULT_AI_CHANNELS.find((d) => d.id === c.id);
        return meta ?? null;
      })
      .filter(Boolean) as typeof DEFAULT_AI_CHANNELS;
  }, [storedChannels]);

  const defaultId = visibleChannels[0]?.id ?? "doubao";
  const [selectedChannelId, setSelectedChannelId] = useState(defaultId);
  const [frameKey, setFrameKey] = useState(0);
  const [isFrameLoading, setIsFrameLoading] = useState(true);
  const deferredLoading = useDeferredValue(isFrameLoading);

  const selectedChannel = useMemo(
    () =>
      visibleChannels.find((c) => c.id === selectedChannelId) ??
      visibleChannels[0],
    [selectedChannelId, visibleChannels],
  );

  return (
    <div className="flex h-full flex-col bg-[#f0f6fc]">

      {/* ── 顶部工具栏 ── */}
      <div className="flex shrink-0 items-center gap-2 border-b border-slate-200/70 bg-white/90 px-3 py-2 backdrop-blur">

        {/* 频道标签 */}
        <div className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto">
          {visibleChannels.map((channel) => {
            const isActive = channel.id === selectedChannel?.id;
            return (
              <button
                key={channel.id}
                type="button"
                onClick={() => {
                  setIsFrameLoading(true);
                  setSelectedChannelId(channel.id);
                  setFrameKey((prev) => prev + 1);
                }}
                title={channel.subtitle}
                className={`flex shrink-0 items-center gap-2 rounded-xl px-3 py-1.5 text-sm transition-all ${
                  isActive
                    ? "bg-slate-900 text-white shadow-sm"
                    : "text-slate-600 hover:bg-slate-100 hover:text-slate-900"
                }`}
              >
                <div
                  className={`flex h-5 w-5 shrink-0 items-center justify-center rounded-md text-[10px] font-bold text-white ${channel.iconClassName} ${channel.iconTextClassName ?? ""}`}
                >
                  {channel.mark}
                </div>
                <span className="font-medium">{channel.title}</span>
              </button>
            );
          })}
        </div>

        {/* 分隔线 */}
        <div className="mx-1 h-5 w-px shrink-0 bg-slate-200" />

        {/* URL 显示 */}
        <div className="flex min-w-0 items-center gap-1.5 rounded-lg bg-slate-100 px-2.5 py-1.5 text-xs text-slate-500 max-w-[220px]">
          <Globe2 className="h-3.5 w-3.5 shrink-0 text-slate-400" />
          <span className="truncate">{selectedChannel?.url}</span>
        </div>

        {/* 操作按钮 */}
        <button
          type="button"
          onClick={() => {
            setIsFrameLoading(true);
            setFrameKey((prev) => prev + 1);
          }}
          className="flex shrink-0 items-center justify-center rounded-lg p-1.5 text-slate-500 transition hover:bg-slate-100 hover:text-slate-800"
          title={t("home.refresh", { defaultValue: "刷新" })}
        >
          <RefreshCw className="h-4 w-4" />
        </button>

        <button
          type="button"
          onClick={onOpenSettings}
          className="flex shrink-0 items-center justify-center rounded-lg p-1.5 text-slate-500 transition hover:bg-slate-100 hover:text-slate-800"
          title={t("home.openSettings", { defaultValue: "设置" })}
        >
          <Settings2 className="h-4 w-4" />
        </button>
      </div>

      {/* ── 内容区 ── */}
      <div className="relative min-h-0 flex-1 overflow-hidden bg-white">
        {selectedChannel && (
          <iframe
            key={frameKey}
            title={t("home.iframeTitle", {
              defaultValue: `${selectedChannel.title} 聊天`,
              title: selectedChannel.title,
            })}
            src={selectedChannel.url}
            className="h-full w-full"
            allow="clipboard-read; clipboard-write; microphone *"
            referrerPolicy="strict-origin-when-cross-origin"
            onLoad={() => setIsFrameLoading(false)}
          />
        )}

        {deferredLoading && (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center bg-white/80 backdrop-blur-sm">
            <div className="flex flex-col items-center gap-3">
              <div className="h-8 w-8 animate-spin rounded-full border-2 border-sky-100 border-t-sky-500" />
              <p className="text-sm font-medium text-slate-600">
                {t("home.loading", { defaultValue: "正在加载..." })}
              </p>
            </div>
          </div>
        )}
      </div>
    </div>
  );
};

export default DoubaoHome;
