/**
 * Grades, speeds and picks from @handy-computer/models (the same catalog as
 * src-tauri/src/catalog/catalog.json).
 */
import {
  accuracyGrade,
  catalog,
  languageIdentity,
  modelMatchesLanguage,
  type CatalogModel,
  type MachineTier,
} from "@handy-computer/models";
import type { GradeLetter } from "@/components/shared/GradeChip";

export type { CatalogModel, MachineTier };

const M = catalog.methodology;

/** Curated speech models. */
export const CURATED_ASR: CatalogModel[] = catalog.curatedModels.filter(
  (m) => m.role === "asr",
);

/** Measured word (or character) error for one language, in percent. */
export const errorFor = (
  model: CatalogModel,
  language: string,
): number | undefined =>
  catalog.data.results[model.id]?.accuracy[languageIdentity(language)]?.error;

export interface Grade {
  letter: GradeLetter;
  /** "A+", "B−"; "—" when the model doesn't speak it, "?" when unmeasured. */
  label: string;
  unsupported?: boolean;
}

/**
 * Max error % per grade (F beyond D). Keep in sync with the package's
 * ACCURACY_CEILINGS, which it doesn't export.
 */
export const GRADE_CEILINGS = { A: 5, B: 10, C: 20, D: 30 } as const;

// The package writes the minus as a hyphen; the design sets a real minus.
const minus = (label: string) => label.replace("-", "−");

export const gradeFor = (model: CatalogModel, language: string): Grade => {
  if (!modelMatchesLanguage(model, language))
    return { letter: "none", label: "—", unsupported: true };
  const grade = catalog.languageAccuracyGrade(model, language);
  if (!grade) return { letter: "none", label: "?" };
  return { letter: grade.letter, label: minus(grade.label) };
};

/** The grade for an error in percent (an average over several languages). */
export const gradeForError = (error: number): Grade => {
  const grade = accuracyGrade(error);
  return { letter: grade.letter, label: minus(grade.label) };
};

/** Seconds after a 30-second dictation, on this machine's tier. */
const waitFor = (model: CatalogModel, tier: MachineTier): number | undefined =>
  catalog.modelWaitSeconds(model, tier);

/** Several languages: the geometric mean of their errors. */
export const geoMean = (xs: number[]) =>
  Math.exp(xs.reduce((a, x) => a + Math.log(x), 0) / xs.length);

/** No curated model hears this language well (under 20% errors). */
export const isWeakLanguage = (language: string): boolean => {
  const errors = CURATED_ASR.filter((m) => modelMatchesLanguage(m, language))
    .map((m) => errorFor(m, language))
    .filter((e): e is number => e != null);
  return !errors.length || Math.min(...errors) >= M.maxLanguageErrorPercent;
};

/* ---------------- the picks ---------------- */

/** Recommended, Faster, More accurate: always in this order. */
export type PickRole = "pick" | "faster" | "accurate";
const ROLE_ORDER: PickRole[] = ["pick", "faster", "accurate"];

interface ModelPick {
  model: CatalogModel;
  role: PickRole;
}

// A model the package gives several roles shows the strongest: Recommended,
// then Faster (true against Recommended), then More accurate.
const PACKAGE_ROLES = [
  ["best", "pick"],
  ["fast", "faster"],
  ["accurate", "accurate"],
] as const;

/** The package's picks for these languages, one per model, limited to `offered`. */
export const picksFor = (
  languages: string[],
  offered: (model: CatalogModel) => boolean,
): ModelPick[] =>
  catalog
    .recommend(languages)
    .filter((r) => offered(r.model))
    .map((r) => {
      const role = PACKAGE_ROLES.find(([p]) => r.roles.includes(p))?.[1];
      return role ? { model: r.model, role } : null;
    })
    .filter((p): p is ModelPick => p !== null)
    .sort((a, b) => ROLE_ORDER.indexOf(a.role) - ROLE_ORDER.indexOf(b.role));

/** Usable models for these languages, ordered by the package's overall cost on this tier. */
const rankFor = (
  languages: string[],
  tier: MachineTier,
  models: CatalogModel[],
): Map<string, number> => {
  const scored = models
    .map((model) => {
      if (!languages.every((l) => modelMatchesLanguage(model, l))) return null;
      const errors = languages.map((l) => errorFor(model, l));
      if (errors.some((e) => e == null)) return null;
      const wait = waitFor(model, tier);
      if (wait == null) return null;
      const error = geoMean(errors as number[]);
      return {
        model,
        worst: Math.max(...(errors as number[])),
        cost: error + M.waitWeight * Math.max(0, wait - M.freeWaitSeconds),
      };
    })
    .filter((s) => s !== null);
  const eligible = scored.filter((s) => s.worst <= M.maxLanguageErrorPercent);
  const pool = (eligible.length ? eligible : scored).sort(
    (a, b) => a.cost - b.cost,
  );
  return new Map(pool.map((s, i) => [s.model.id, i]));
};

/* ---------------- the full list ---------------- */

export interface ModelRow {
  model: CatalogModel;
  /** Its role when it's one of the picks. */
  role: PickRole | null;
  /** Your languages it doesn't speak. */
  missing: string[];
  /** Average error over your languages, when every one is measured. */
  error: number | null;
  wait: number | undefined;
}

interface ModelGroups {
  /** The picks, in order. */
  picks: ModelRow[];
  /** The rest that speak every one of your languages, best value first. */
  others: ModelRow[];
  /** Models missing one of your languages: fewest missing, then accuracy. */
  partial: ModelRow[];
}

export const groupModels = (
  languages: string[],
  tier: MachineTier,
  models: CatalogModel[],
  picks: ModelPick[],
): ModelGroups => {
  const roles = new Map(picks.map((p) => [p.model.id, p.role]));
  const rank = rankFor(languages, tier, models);
  const rows: ModelRow[] = models.map((model) => {
    const errors = languages.map((l) => errorFor(model, l));
    return {
      model,
      role: roles.get(model.id) ?? null,
      missing: languages.filter((l) => !modelMatchesLanguage(model, l)),
      error: errors.every((e) => e != null)
        ? geoMean(errors as number[])
        : null,
      wait: waitFor(model, tier),
    };
  });
  const score = (r: ModelRow) => rank.get(r.model.id) ?? 100 + (r.error ?? 99);
  return {
    picks: picks
      .map((p) => rows.find((r) => r.model.id === p.model.id))
      .filter((r): r is ModelRow => !!r),
    others: rows
      .filter((r) => !r.role && !r.missing.length)
      .sort((a, b) => score(a) - score(b)),
    partial: rows
      .filter((r) => !r.role && r.missing.length)
      .sort(
        (a, b) =>
          a.missing.length - b.missing.length ||
          (a.error ?? 99) - (b.error ?? 99),
      ),
  };
};

/* ---------------- speed on this computer (Try it) ---------------- */

/** Slower than the overall pick's budget; takes under 5 s aren't judged. */
export const isTooSlow = (speechSeconds: number, waitSeconds: number) =>
  speechSeconds >= 5 &&
  speechSeconds / Math.max(waitSeconds, 0.05) <
    M.dictationSeconds / M.overallMaxWaitSeconds;
