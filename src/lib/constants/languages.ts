/**
 * Spoken languages from the model catalog. Codes are `languageIdentity`s,
 * one spelling per language (no/nb → nb), compared and saved as is.
 */
import { catalog, languageIdentity } from "@handy-computer/models";

/** `native` is the endonym, never translated. UI names: `languageName`. */
export interface Language {
  code: string;
  native: string;
}

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

/** Languages' names as a list in the UI language ("English and Chinese"). */
export const languageList = (codes: string[], uiLanguage: string): string =>
  new Intl.ListFormat(uiLanguage, { type: "conjunction" }).format(
    codes.map((code) => languageName(code, uiLanguage)),
  );

// Scripts that capitalize a name's first letter. (Georgian has capitals but
// doesn't start words with them.)
const CASED =
  /^[\p{Script=Latin}\p{Script=Cyrillic}\p{Script=Greek}\p{Script=Armenian}]/u;

/** A language's name in itself, capitalized as a heading ("Français"). */
const endonym = (code: string): string => {
  const name = languageName(code, code);
  return CASED.test(name)
    ? name.charAt(0).toLocaleUpperCase(code) + name.slice(1)
    : name;
};

// How many of the catalog's speech models hear each language.
const SUPPORT = new Map<string, number>();
for (const model of catalog.models.filter((m) => m.role === "asr"))
  for (const code of new Set(model.languages.map(languageIdentity)))
    SUPPORT.set(code, (SUPPORT.get(code) ?? 0) + 1);

const byEnglishName = (a: string, b: string) =>
  languageName(a, "en").localeCompare(languageName(b, "en"));

/** Catalog languages, most supported first, then by English name. */
export const LANGUAGES: Language[] = [...SUPPORT]
  .sort(
    ([a, aModels], [b, bModels]) => bModels - aModels || byEnglishName(a, b),
  )
  .map(([code]) => ({ code, native: endonym(code) }));

const BY_CODE = new Map(LANGUAGES.map((l) => [l.code, l]));
const RANK = new Map(LANGUAGES.map((l, i) => [l.code, i]));

/** The language for any code a model or setting uses ("nb-NO", "jw"). */
export const getLanguage = (code: string): Language => {
  const identity = languageIdentity(code);
  return BY_CODE.get(identity) ?? { code: identity, native: endonym(identity) };
};

/** A model's languages, deduped, in catalog order; unknown codes go last. */
export const languagesOf = (codes: readonly string[]): Language[] =>
  [...new Set(codes.map(languageIdentity))]
    .sort(
      (a, b) =>
        (RANK.get(a) ?? Infinity) - (RANK.get(b) ?? Infinity) ||
        byEnglishName(a, b),
    )
    .map(getLanguage);

/** Whether a model's codes include this language, however it's spelled. */
export const supportsLanguage = (
  codes: readonly string[],
  language: string,
): boolean => {
  const identity = languageIdentity(language);
  return codes.some((code) => languageIdentity(code) === identity);
};

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

/** Languages matching a query; a prefix match on any name beats a substring. */
export const searchLanguages = (
  query: string,
  uiLanguage: string,
  languages: readonly Language[] = LANGUAGES,
): Language[] => {
  const q = query.trim().toLowerCase();
  if (!q) return languages.slice();
  const hits: [number, Language][] = [];
  for (const l of languages) {
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
