export interface AiChannel {
  id: string;
  title: string;
  subtitle: string;
  url: string;
  mark: string;
  iconClassName: string;
  iconTextClassName?: string;
}

export const DEFAULT_AI_CHANNELS: AiChannel[] = [
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
    id: "openai",
    title: "ChatGPT",
    subtitle: "OpenAI",
    url: "https://chatgpt.com/",
    mark: "AI",
    iconClassName: "bg-[linear-gradient(135deg,#10a37f_0%,#0e7c63_100%)]",
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
