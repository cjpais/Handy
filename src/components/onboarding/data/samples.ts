/**
 * What Demo's animation "says": a line in the person's own language, so they
 * see their language land. This is dictation, not UI copy, so it's written
 * in each language rather than translated.
 */
import { isChinese, type ChineseScriptChoice } from "./languages";

export const SAMPLES: Record<string, string> = {
  es: "Oye, ¿podemos mover la revisión de diseño de mañana a las tres? Quiero un poco más de tiempo para terminar las maquetas.",
  fr: "Est-ce qu'on peut décaler la revue de design de demain à quinze heures ? J'aimerais un peu plus de temps pour finir les maquettes.",
  de: "Können wir die Design-Besprechung morgen auf drei Uhr verschieben? Ich brauche noch etwas Zeit für die Entwürfe.",
  ja: "明日のデザインレビューを三時に変更できますか？モックアップを仕上げる時間が少し欲しいです。",
  ko: "내일 디자인 리뷰를 세 시로 옮길 수 있을까요? 시안을 마무리할 시간이 조금 더 필요해요.",
  "zh-Hans": "我们明天的设计评审能改到三点吗？我想多一点时间把原型做完。",
  "zh-Hant": "我們明天的設計評審能改到三點嗎？我想多一點時間把原型做完。",
  "zh-HK": "我哋聽日嘅設計評審可唔可以改到三點？我想多啲時間做完個原型。",
  pt: "Podemos mudar a revisão de design de amanhã para as três? Quero um pouco mais de tempo para terminar os protótipos.",
  ru: "Можем перенести завтрашний дизайн-ревью на три часа? Хочу ещё немного времени, чтобы доделать макеты.",
  it: "Possiamo spostare la revisione del design di domani alle tre? Vorrei un po' più di tempo per finire i prototipi.",
};

// English gets its own line per app: hold answers the friend in Messages,
// tap (for anything longer) fills a note.
const ENGLISH = {
  hold: "Yes! Running ten minutes late, save me a seat.",
  tap: "Picnic in the park on Saturday, then dinner at Rosa's. Bring the blue blanket and the good speaker.",
};

/**
 * The line for one way of using the shortcut: hold shows the first spoken
 * language, tap the second (or the first again). Languages without a sample
 * fall back to English.
 */
export const demoLine = (
  languages: string[],
  script: ChineseScriptChoice,
  mode: "hold" | "tap",
): { language: string; text: string } => {
  const i = mode === "hold" ? 0 : 1;
  const langs = languages.length ? languages : ["en"];
  const language = langs[i % langs.length];
  const sample = isChinese(language) ? SAMPLES[script] : SAMPLES[language];
  if (!sample) return { language: "en", text: ENGLISH[mode] };
  const sentences = sample.split(/(?<=[?？。！!.])\s*/).filter(Boolean);
  return { language, text: sentences[i % sentences.length] };
};
