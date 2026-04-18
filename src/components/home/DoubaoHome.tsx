import { Globe2, RefreshCw, Settings2 } from "lucide-react";
import { useDeferredValue, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

interface AiChannel {
  id: string;
  title: string;
  subtitle: string;
  url: string;
  mark: string;
  iconClassName: string;
  iconTextClassName?: string;
}

const AI_CHANNELS: AiChannel[] = [
  {
    id: "deepseek",
    title: "DeepSeek",
    subtitle: "0528 新版",
    url: "https://chat.deepseek.com/",
    mark: "DS",
    iconClassName: "bg-[linear-gradient(135deg,#595cff_0%,#8d5bff_100%)]",
  },
  {
    id: "doubao",
    title: "豆包",
    subtitle: "seed-1.6",
    url: "https://www.doubao.com/chat/?",
    mark: "豆",
    iconClassName: "bg-[linear-gradient(135deg,#ffd7e2_0%,#bedcff_100%)]",
    iconTextClassName: "text-slate-800",
  },
  {
    id: "kimi",
    title: "Kimi",
    subtitle: "K2-0711",
    url: "https://kimi.moonshot.cn/",
    mark: "K",
    iconClassName: "bg-[linear-gradient(135deg,#09090b_0%,#27272a_100%)]",
  },
  {
    id: "tongyi",
    title: "通义千问",
    subtitle: "Qwen",
    url: "https://tongyi.aliyun.com/",
    mark: "Q",
    iconClassName: "bg-[linear-gradient(135deg,#8b7bff_0%,#6170ff_100%)]",
  },
  {
    id: "yuanbao",
    title: "腾讯元宝",
    subtitle: "Standard",
    url: "https://yuanbao.tencent.com/",
    mark: "元",
    iconClassName: "bg-[linear-gradient(135deg,#4fd1a1_0%,#72d6f7_100%)]",
  },
  {
    id: "ernie",
    title: "文心一言",
    subtitle: "Turbo-32K",
    url: "https://ernie.baidu.com/",
    mark: "文",
    iconClassName: "bg-[linear-gradient(135deg,#4f8cff_0%,#1c57d6_100%)]",
  },
  {
    id: "spark",
    title: "讯飞星火",
    subtitle: "深度推理 X1",
    url: "https://xinghuo.xfyun.cn/",
    mark: "星",
    iconClassName: "bg-[linear-gradient(135deg,#3bb6ff_0%,#ff6a6a_100%)]",
  },
  {
    id: "minimax",
    title: "MiniMax",
    subtitle: "abab 6.5s",
    url: "https://www.minimax.io/",
    mark: "M",
    iconClassName: "bg-[linear-gradient(135deg,#ff4d77_0%,#ff8a3d_100%)]",
  },
];

interface DoubaoHomeProps {
  onOpenSettings: () => void;
}

const DoubaoHome: React.FC<DoubaoHomeProps> = ({ onOpenSettings }) => {
  const { t } = useTranslation();
  const [selectedChannelId, setSelectedChannelId] = useState("doubao");
  const [frameKey, setFrameKey] = useState(0);
  const [isFrameLoading, setIsFrameLoading] = useState(true);
  const deferredLoading = useDeferredValue(isFrameLoading);
  const selectedChannel = useMemo(
    () =>
      AI_CHANNELS.find((channel) => channel.id === selectedChannelId) ??
      AI_CHANNELS[0],
    [selectedChannelId],
  );

  return (
    <div className="relative h-full w-full overflow-hidden bg-[#d9eefb]">
      <div className="absolute inset-0 bg-[radial-gradient(circle_at_top_left,_rgba(255,255,255,0.96),_transparent_38%),linear-gradient(140deg,_#eef8ff_0%,_#d8edf9_50%,_#edf7ff_100%)]" />

      <div className="relative flex h-full flex-col p-4">
        <div className="mb-3 rounded-[28px] border border-white/80 bg-white/72 p-3 shadow-[0_18px_60px_rgba(84,116,146,0.12)] backdrop-blur">
          <div className="flex gap-3 overflow-x-auto pb-1">
            {AI_CHANNELS.map((channel) => {
              const isActive = channel.id === selectedChannel.id;

              return (
                <button
                  key={channel.id}
                  type="button"
                  onClick={() => {
                    setIsFrameLoading(true);
                    setSelectedChannelId(channel.id);
                    setFrameKey((prev) => prev + 1);
                  }}
                  className={`group min-w-[168px] rounded-[24px] border px-4 py-3 text-left transition ${
                    isActive
                      ? "border-slate-900 bg-white shadow-[0_14px_40px_rgba(15,23,42,0.12)]"
                      : "border-slate-200/80 bg-white/72 hover:border-sky-200 hover:bg-white"
                  }`}
                >
                  <div className="flex items-center gap-3">
                    <div
                      className={`flex h-12 w-12 shrink-0 items-center justify-center rounded-2xl text-lg font-bold text-white shadow-[0_10px_24px_rgba(15,23,42,0.14)] ${channel.iconClassName} ${channel.iconTextClassName ?? ""}`}
                    >
                      {channel.mark}
                    </div>
                    <div className="min-w-0">
                      <p className="truncate text-base font-semibold text-slate-900">
                        {channel.title}
                      </p>
                      <p className="truncate text-xs text-slate-500">
                        {channel.subtitle}
                      </p>
                    </div>
                  </div>
                </button>
              );
            })}
          </div>
        </div>

        <div className="mb-3 flex items-center justify-between gap-3 rounded-3xl border border-white/70 bg-white/80 px-4 py-3 shadow-[0_18px_60px_rgba(84,116,146,0.12)] backdrop-blur">
          <div className="min-w-0">
            <div className="flex items-center gap-2 text-sm font-semibold text-slate-800">
              <Globe2 className="h-4 w-4 text-sky-600" />
              <span>
                {selectedChannel.title}
                {t("home.titleSuffix", { defaultValue: "主页" })}
              </span>
            </div>
            <p className="truncate text-xs text-slate-500">
              {selectedChannel.url}
            </p>
          </div>

          <button
            type="button"
            onClick={() => {
              setIsFrameLoading(true);
              setFrameKey((prev) => prev + 1);
            }}
            className="inline-flex shrink-0 items-center gap-2 rounded-full border border-slate-200 bg-white px-3 py-2 text-xs font-medium text-slate-700 transition hover:border-sky-300 hover:text-sky-700"
          >
            <RefreshCw className="h-3.5 w-3.5" />
            <span>{t("home.refresh", { defaultValue: "刷新" })}</span>
          </button>
        </div>

        <div className="relative min-h-0 flex-1 overflow-hidden rounded-[28px] border border-white/80 bg-white shadow-[0_22px_90px_rgba(52,84,117,0.16)]">
          <iframe
            key={frameKey}
            title={t("home.iframeTitle", {
              defaultValue: `${selectedChannel.title} 聊天`,
            })}
            src={selectedChannel.url}
            className="h-full w-full bg-white"
            allow="clipboard-read; clipboard-write; microphone *"
            referrerPolicy="strict-origin-when-cross-origin"
            onLoad={() => setIsFrameLoading(false)}
          />

          {deferredLoading ? (
            <div className="pointer-events-none absolute inset-0 flex items-center justify-center bg-white/78 backdrop-blur-sm">
              <div className="rounded-3xl border border-sky-100 bg-white/95 px-6 py-5 text-center shadow-[0_18px_50px_rgba(50,94,138,0.12)]">
                <div className="mx-auto mb-3 h-10 w-10 animate-spin rounded-full border-2 border-sky-100 border-t-sky-500" />
                <p className="text-sm font-medium text-slate-800">
                  {t("home.loading", { defaultValue: "正在加载豆包主页..." })}
                </p>
                <p className="mt-1 text-xs text-slate-500">
                  {t("home.loadingHint", {
                    defaultValue: "首次加载可能会稍慢一些",
                  })}
                </p>
              </div>
            </div>
          ) : null}
        </div>

        <div className="mt-3 flex items-center justify-end rounded-[28px] border border-white/80 bg-white/86 px-4 py-3 shadow-[0_18px_60px_rgba(52,84,117,0.12)] backdrop-blur">
          <button
            type="button"
            onClick={onOpenSettings}
            className="inline-flex items-center gap-2 rounded-full bg-slate-950 px-5 py-3 text-sm font-medium text-white shadow-[0_18px_40px_rgba(15,23,42,0.2)] transition hover:bg-slate-800"
          >
            <Settings2 className="h-4 w-4" />
            <span>{t("home.openSettings", { defaultValue: "设置" })}</span>
          </button>
        </div>
      </div>
    </div>
  );
};

export default DoubaoHome;
