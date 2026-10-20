/**
 * The spoken languages offered on Welcome. `native` is the endonym, shown
 * first so people find their own language in their own script; it's the
 * same in every UI language, so it isn't translated. The name under it comes
 * from Intl in the UI language.
 */
export interface SpokenLanguage {
  code: string;
  native: string;
}

export const SPOKEN_LANGUAGES: SpokenLanguage[] = [
  { code: "en", native: "English" },
  { code: "zh", native: "中文" },
  { code: "es", native: "Español" },
  { code: "hi", native: "हिन्दी" },
  { code: "ar", native: "العربية" },
  { code: "fr", native: "Français" },
  { code: "pt", native: "Português" },
  { code: "ru", native: "Русский" },
  { code: "ja", native: "日本語" },
  { code: "de", native: "Deutsch" },
  { code: "ko", native: "한국어" },
  { code: "it", native: "Italiano" },
  { code: "tr", native: "Türkçe" },
  { code: "vi", native: "Tiếng Việt" },
  { code: "yue", native: "粵語" },
  { code: "pl", native: "Polski" },
  { code: "uk", native: "Українська" },
  { code: "nl", native: "Nederlands" },
  { code: "id", native: "Bahasa Indonesia" },
  { code: "th", native: "ไทย" },
  { code: "el", native: "Ελληνικά" },
  { code: "sv", native: "Svenska" },
  { code: "cs", native: "Čeština" },
  { code: "ro", native: "Română" },
  { code: "hu", native: "Magyar" },
  { code: "da", native: "Dansk" },
  { code: "fi", native: "Suomi" },
  { code: "nb", native: "Norsk" },
  { code: "he", native: "עברית" },
  { code: "fa", native: "فارسی" },
  { code: "ms", native: "Bahasa Melayu" },
  { code: "fil", native: "Filipino" },
];

const BY_CODE = new Map(SPOKEN_LANGUAGES.map((l) => [l.code, l]));
export const spokenLanguage = (code: string) => BY_CODE.get(code);

export const isChinese = (code: string) => code === "zh" || code === "yue";

// What people might type that isn't the endonym or the name.
const ALIASES: Record<string, string[]> = {
  zh: [
    "chinese",
    "mandarin",
    "putonghua",
    "guoyu",
    "普通话",
    "國語",
    "汉语",
    "華語",
    "中文",
  ],
  yue: ["cantonese", "广东话", "廣東話", "粤语", "粵語"],
  fa: ["farsi"],
  fil: ["tagalog", "pilipino"],
  nb: ["norwegian", "bokmål", "bokmal"],
  ms: ["malaysian", "melayu"],
  id: ["indonesian", "bahasa"],
  he: ["ivrit"],
  hi: ["hindustani"],
  ja: ["nihongo"],
  ko: ["hangul", "hangugeo"],
};

// Intl names "nb" Norwegian Bokmål; people say Norwegian.
const NAME_CODES: Record<string, string> = { nb: "no" };

/** A language's name in the UI language ("Chinese", "Chinesisch"). */
export const languageName = (code: string, uiLanguage: string): string => {
  try {
    const names = new Intl.DisplayNames([uiLanguage, "en"], {
      type: "language",
    });
    return names.of(NAME_CODES[code] ?? code) ?? code;
  } catch {
    return code;
  }
};

/**
 * Languages matching what's typed, best first: a prefix of any name (the
 * endonym, the name in the UI language or English, the code, an alias) beats
 * a match inside one.
 */
export const searchLanguages = (
  query: string,
  uiLanguage: string,
): SpokenLanguage[] => {
  const q = query.trim().toLowerCase();
  if (!q) return SPOKEN_LANGUAGES.slice();
  const hits: [number, SpokenLanguage][] = [];
  for (const l of SPOKEN_LANGUAGES) {
    const names = [
      l.native,
      languageName(l.code, uiLanguage),
      languageName(l.code, "en"),
      l.code,
      ...(ALIASES[l.code] ?? []),
    ].map((n) => n.toLowerCase());
    const prefix = names.some((n) => n.startsWith(q));
    if (prefix || names.some((n) => n.includes(q)))
      hits.push([prefix ? 0 : 1, l]);
  }
  return hits.sort((a, b) => a[0] - b[0]).map(([, l]) => l);
};

/** The OS locale's language, if it's one we offer; else English. */
export const suggestedLanguages = (locale: string): string[] => {
  const base = locale.toLowerCase().split(/[-_]/)[0];
  return BY_CODE.has(base) ? [base] : ["en"];
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
