import { languageIdentity } from "@handy-computer/models";
import { LANGUAGES } from "@/lib/constants/languages";

export const isChinese = (code: string) => code === "zh" || code === "yue";

/** The OS locale's language, if it's one we offer; else English. */
export const suggestedLanguages = (locale: string): string[] => {
  const code = languageIdentity(locale.split("_")[0]);
  return LANGUAGES.some((l) => l.code === code) ? [code] : ["en"];
};

/** The Chinese characters Handy writes. */
export type ChineseScriptChoice = "zh-Hans" | "zh-Hant" | "zh-HK";

export const CHINESE_SCRIPTS: {
  id: ChineseScriptChoice;
  /** The script's own name, not translated. */
  native: string;
  /** For the lang attribute, so the glyphs render in the right style. */
  lang: string;
}[] = [
  { id: "zh-Hans", native: "简体", lang: "zh-CN" },
  { id: "zh-Hant", native: "繁體", lang: "zh-TW" },
  { id: "zh-HK", native: "繁體", lang: "zh-HK" },
];

export const suggestedScript = (locale: string): ChineseScriptChoice => {
  const l = locale.toLowerCase();
  if (/hk|mo/.test(l)) return "zh-HK";
  if (/tw|hant/.test(l)) return "zh-Hant";
  return "zh-Hans";
};
