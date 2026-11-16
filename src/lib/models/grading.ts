import {
  catalog,
  modelMatchesLanguage,
  type CatalogModel,
  type MachineTier,
} from "@handy-computer/models";
import {
  errorFor,
  geoMean,
  gradeFor,
  gradeForError,
  type Grade,
  type PickRole,
} from "./catalog";

/**
 * Grades models for the user's languages on this machine's tier. Shared by
 * onboarding and the Models page; models without a catalog entry are ungraded.
 */
export interface Grading {
  langs: string[];
  tier: MachineTier;
  roleOf: (m: CatalogModel | undefined) => PickRole | null;
  grade: (m: CatalogModel | undefined, language: string) => Grade;
  /** One grade over all your languages; "—" when it misses one. */
  overall: (m: CatalogModel | undefined) => Grade;
  wait: (m: CatalogModel | undefined) => number | undefined;
  /** Your languages it doesn't hear. */
  missing: (m: CatalogModel | undefined) => string[];
}

const UNGRADED: Grade = { letter: "none", label: "?" };

export const makeGrading = (
  langs: string[],
  tier: MachineTier,
  picks: { model: CatalogModel; role: PickRole }[],
): Grading => {
  const roles = new Map(picks.map((p) => [p.model.id, p.role]));
  const missing = (m: CatalogModel | undefined) =>
    m ? langs.filter((l) => !modelMatchesLanguage(m, l)) : [];
  return {
    langs,
    tier,
    roleOf: (m) => (m && roles.get(m.id)) ?? null,
    grade: (m, l) => (m ? gradeFor(m, l) : UNGRADED),
    overall: (m) => {
      if (!m) return UNGRADED;
      if (missing(m).length) return { letter: "none", label: "—" };
      const error = averageError(m, langs);
      return error == null ? UNGRADED : gradeForError(error);
    },
    wait: (m) => (m ? catalog.modelWaitSeconds(m, tier) : undefined),
    missing,
  };
};

/** The average error over these languages, when every one is measured. */
export const averageError = (
  m: CatalogModel,
  langs: string[],
): number | null => {
  const errors = langs.map((l) => errorFor(m, l));
  return errors.every((e) => e != null) ? geoMean(errors as number[]) : null;
};
