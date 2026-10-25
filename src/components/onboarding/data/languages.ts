import { languageIdentity } from "@handy-computer/models";
import type { ChineseScript } from "@/bindings";
import { LANGUAGES } from "@/lib/constants/languages";

declare global {
  interface Window {
    /**
     * The guesses from the OS's preferred languages (`system_languages.rs`),
     * set before any script runs. Missing outside the app (a plain browser).
     */
    __HANDY_SYSTEM_LANGUAGES__?: {
      languages: string[];
      chinese_script: ChineseScript | null;
    };
  }
}

const system = () => window.__HANDY_SYSTEM_LANGUAGES__;
const locale = () => navigator.language || "en-US";

export const isChinese = (code: string) => code === "zh" || code === "yue";

/**
 * The languages someone likely speaks, from the OS's preferred languages,
 * with English among them: the same guess the settings migration backfills.
 * Outside the app, the browser locale's language.
 */
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

/**
 * The script their OS's first Chinese language names (`zh-Hant-US` →
 * Traditional), else the browser locale's, else Simplified.
 */
export const suggestedScript = (): ChineseScriptChoice => {
  const script = system()?.chinese_script;
  const guessed = script ? SCRIPT_CHOICE[script] : undefined;
  if (guessed) return guessed;
  const l = locale().toLowerCase();
  if (/hk|mo/.test(l)) return "zh-HK";
  if (/tw|hant/.test(l)) return "zh-Hant";
  return "zh-Hans";
};
