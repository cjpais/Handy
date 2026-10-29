import { languageIdentity } from "@handy-computer/models";
import type { ChineseScript } from "@/bindings";
import { LANGUAGES } from "@/lib/constants/languages";

declare global {
  interface Window {
    /** Injected before scripts run (`system_languages.rs`); absent in a browser. */
    __HANDY_SYSTEM_LANGUAGES__?: {
      languages: string[];
      chinese_script: ChineseScript | null;
    };
  }
}

const system = () => window.__HANDY_SYSTEM_LANGUAGES__;
const locale = () => navigator.language || "en-US";

export const isChinese = (code: string) => code === "zh" || code === "yue";

/** Likely spoken languages (always with English), falling back to the browser locale. */
export const suggestedLanguages = (): string[] => {
  const guessed = system()?.languages;
  if (guessed?.length) return guessed;
  const code = languageIdentity(locale().split("_")[0]);
  return code !== "en" && LANGUAGES.some((l) => l.code === code)
    ? [code, "en"]
    : ["en"];
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

const SCRIPT_CHOICE: Partial<Record<ChineseScript, ChineseScriptChoice>> = {
  simplified: "zh-Hans",
  traditional_tw: "zh-Hant",
  traditional_hk: "zh-HK",
};

/** Chinese script from the OS's languages, else the browser locale, else Simplified. */
export const suggestedScript = (): ChineseScriptChoice => {
  const script = system()?.chinese_script;
  const guessed = script ? SCRIPT_CHOICE[script] : undefined;
  if (guessed) return guessed;
  const l = locale().toLowerCase();
  if (/hk|mo/.test(l)) return "zh-HK";
  if (/tw|hant/.test(l)) return "zh-Hant";
  return "zh-Hans";
};
