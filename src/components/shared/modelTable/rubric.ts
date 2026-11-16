import { useTranslation } from "react-i18next";
import { SPEED_CUTOFFS } from "@/components/shared/SpeedBars";
import type { GradeLetter } from "@/components/shared/GradeChip";
import { GRADE_CEILINGS } from "@/lib/models/catalog";

/** The band one grade or speed-bar count stands for, e.g. "5–10%" or "under 2 s". */
export const useRubric = () => {
  const { t, i18n } = useTranslation();
  const n = (x: number) =>
    x.toLocaleString(i18n.language, { maximumFractionDigits: 1 });
  const key = (k: string) => `modelTable.rubric.${k}`;

  const letters = Object.keys(
    GRADE_CEILINGS,
  ) as (keyof typeof GRADE_CEILINGS)[];
  const gradeBand = (letter: Exclude<GradeLetter, "none">) => {
    if (letter === "F") return t(key("over"), { min: n(GRADE_CEILINGS.D) });
    const i = letters.indexOf(letter);
    return i
      ? t(key("between"), {
          min: n(GRADE_CEILINGS[letters[i - 1]]),
          max: n(GRADE_CEILINGS[letter]),
        })
      : t(key("under"), { max: n(GRADE_CEILINGS[letter]) });
  };

  // SPEED_CUTOFFS runs fastest to slowest: four bars beat all of them.
  const speedBand = (level: number) => {
    const i = SPEED_CUTOFFS.length - level;
    if (i >= SPEED_CUTOFFS.length)
      return t(key("overSeconds"), {
        min: n(SPEED_CUTOFFS[SPEED_CUTOFFS.length - 1]),
      });
    return i
      ? t(key("betweenSeconds"), {
          min: n(SPEED_CUTOFFS[i - 1]),
          max: n(SPEED_CUTOFFS[i]),
        })
      : t(key("underSeconds"), { max: n(SPEED_CUTOFFS[i]) });
  };

  return { gradeBand, speedBand, number: n };
};
