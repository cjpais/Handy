import type {
  BackfillStatus,
  Fillers,
  Openers,
  Phrases,
  Vocabulary,
  WordCount,
  WordDetail,
  WordsPeriod,
} from "@/bindings";

/** Fixture data for the Words tab (`/voxa-preview`), deterministic. */

// [stem, word, count in a week, function word?]
const WORDS: [string, string, number, boolean?][] = [
  ["промпт", "промпт", 41],
  ["модел", "модель", 37],
  ["дан", "данные", 34],
  ["пайплайн", "пайплайн", 29],
  ["claude", "claude", 27],
  ["агент", "агент", 24],
  ["сдела", "сделать", 22],
  ["эмбеддинг", "эмбеддинги", 19],
  ["проверя", "проверить", 18],
  ["запрос", "запрос", 17],
  ["контекст", "контекст", 16],
  ["поиск", "поиск", 15],
  ["реран", "реранкер", 13],
  ["дашборд", "дашборд", 12],
  ["обсидиан", "obsidian", 11],
  ["докер", "docker", 10],
  ["отчет", "отчёт", 10],
  ["задач", "задача", 9],
  ["тест", "тест", 9],
  ["команд", "команда", 8],
  ["и", "и", 312, true],
  ["в", "в", 244, true],
  ["что", "что", 160, true],
  ["это", "это", 148, true],
  ["на", "на", 141, true],
  ["не", "не", 133, true],
  ["я", "я", 120, true],
  ["так", "так", 96, true],
];

// period -> multiplier of the week numbers
function factor(period: WordsPeriod): number {
  if (period.from === null) return 12;
  if (period.to !== null && period.from !== null) {
    const days =
      (Date.parse(period.to) - Date.parse(period.from)) / 86_400_000 + 1;
    return Math.max(1, Math.round(days / 7));
  }
  return 1;
}

export function topWords(
  period: WordsPeriod,
  includeStop: boolean,
): WordCount[] {
  const k = factor(period);
  return WORDS.filter(([, , , stop]) => includeStop || !stop)
    .map(([stem, word, count]) => ({ stem, word, count: count * k }))
    .sort((a, b) => b.count - a.count);
}

export function newWords(): WordCount[] {
  return [
    ["колбэк", "колбэк", 5],
    ["векторизация", "векторизация", 4],
    ["тюнинг", "тюнинг", 4],
    ["ретривер", "ретривер", 3],
    ["чанкинг", "чанкинг", 3],
    ["логирование", "логирование", 2],
    ["бэкфилл", "бэкфилл", 2],
  ].map(([stem, word, count]) => ({
    stem: stem as string,
    word: word as string,
    count: count as number,
  }));
}

export function vocabulary(period: WordsPeriod): Vocabulary {
  const k = factor(period);
  return {
    distinct: Math.round(420 * Math.sqrt(k) * 1.5),
    total: 2_900 * k,
  };
}

export function openers(period: WordsPeriod): Openers {
  const k = factor(period);
  const total = 84 * k;
  const pick = (text: string, share: number) => ({
    text,
    count: Math.round(total * share),
    percent: share * 100,
  });
  return {
    total,
    one: [
      pick("короче", 0.21),
      pick("давай", 0.14),
      pick("слушай", 0.09),
      pick("так", 0.08),
      pick("смотри", 0.06),
      pick("значит", 0.05),
    ],
    two: [
      pick("давай посмотрим", 0.07),
      pick("короче давай", 0.06),
      pick("слушай а", 0.04),
      pick("смотри тут", 0.035),
      pick("так вот", 0.03),
    ],
  };
}

export function phrases(period: WordsPeriod): Phrases {
  const k = factor(period);
  const rows = (items: [string, number][]) =>
    items.map(([text, count]) => ({ text, count: count * k }));
  return {
    bigrams: rows([
      ["сделай так", 14],
      ["в итоге", 11],
      ["по сути", 9],
      ["давай посмотрим", 8],
      ["в общем", 7],
    ]),
    trigrams: rows([
      ["сделай так чтобы", 8],
      ["давай сделаем так", 5],
      ["как мне кажется", 4],
    ]),
  };
}

const FILLERS: [string, number[]][] = [
  ["короче", [31, 29, 33, 27, 25, 26, 22, 21, 19, 18, 16, 14]],
  ["ну", [24, 22, 21, 22, 19, 18, 17, 16, 15, 15, 12, 11]],
  ["как бы", [14, 15, 12, 13, 11, 10, 11, 9, 8, 8, 7, 6]],
  ["типа", [12, 11, 11, 9, 9, 8, 7, 7, 6, 5, 5, 4]],
  ["вот", [10, 9, 9, 8, 8, 7, 7, 6, 6, 5, 5, 5]],
  ["значит", [6, 5, 5, 5, 4, 4, 4, 3, 3, 3, 2, 2]],
  ["блин", [3, 2, 3, 2, 2, 1, 2, 1, 1, 1, 0, 1]],
  ["э", [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]],
];

export function fillers(period: WordsPeriod): Fillers {
  const weeks = Array.from({ length: 12 }, (_, i) => {
    const monday = new Date(Date.UTC(2026, 6, 27 + i * 7));
    const week_start = monday.toISOString().slice(0, 10);
    const fillerTotal = FILLERS.reduce((sum, [, w]) => sum + w[i], 0);
    return { week_start, fillers: fillerTotal, words: 2_700 + (i % 4) * 150 };
  });
  const span = period.from === null ? 12 : Math.min(12, factor(period));
  return {
    items: FILLERS.map(([word, weekly]) => ({
      word,
      weekly,
      count: weekly.slice(-span).reduce((a, b) => a + b, 0),
    })).sort((a, b) => b.count - a.count),
    weeks,
  };
}

export function wordDetail(stem: string): WordDetail {
  const base = WORDS.find((w) => w[0] === stem);
  const word = base?.[1] ?? stem;
  const forms = [
    { form: word, count: 18 },
    { form: `${word}а`, count: 9 },
    { form: `${word}ом`, count: 6 },
    { form: `${word}ы`, count: 3 },
  ];
  const example = (id: number, before: string, after: string) => ({
    dictation_id: id,
    created_at: 1_792_300_000 - id * 86_400,
    segments: [
      { text: before, hit: false },
      { text: word, hit: true },
      { text: after, hit: false },
    ],
  });
  return {
    word,
    total: forms.reduce((s, f) => s + f.count, 0),
    forms,
    examples: [
      example(
        5,
        "Давай поправим ",
        " и перезапустим пайплайн на новых данных.",
      ),
      example(
        4,
        "Короче, главное — чтобы ",
        " не ломался на длинном контексте.",
      ),
      example(3, "Я сегодня переписал ", ", теперь он стабильнее."),
      example(2, "Слушай, а можно сделать ", " чуть короче?"),
      example(1, "Проверь ещё раз ", " перед отправкой отчёта."),
    ],
  };
}

export function backfillStatus(kind: "running" | "done"): BackfillStatus {
  return kind === "running"
    ? { state: "running", processed: 1_200, total: 5_000 }
    : { state: "done", processed: 0, total: 0 };
}
