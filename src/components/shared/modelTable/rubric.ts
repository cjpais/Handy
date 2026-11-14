import { useTranslation } from "react-i18next";
import { SPEED_CUTOFFS } from "@/components/shared/SpeedBars";

/** The band one speed-bar count stands for, e.g. "under 2 s". */
export const useRubric = () => {
  const { t, i18n } = useTranslation();
  const n = (x: number) =>
    x.toLocaleString(i18n.language, { maximumFractionDigits: 1 });
  const key = (k: string) => `modelTable.rubric.${k}`;

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

  return { speedBand, number: n };
};
