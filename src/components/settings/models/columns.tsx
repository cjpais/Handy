import React from "react";
import { useTranslation } from "react-i18next";
import { languageName } from "@/lib/constants/languages";

/**
 * A grade for each language up to three; four and five take onboarding's
 * dense chips; past five the grades fold into one overall grade per row, as
 * at the Settings width (about 540px) a sixth column leaves the name no
 * room. Hovering the overall grade lists each language's.
 */
const MAX_COLUMNS = 5;
const MAX_ROOMY = 3;

export const gradeMode = (langs: string[]) =>
  langs.length > MAX_COLUMNS ? "overall" : "each";
export const isDense = (langs: string[]) =>
  langs.length > MAX_ROOMY && langs.length <= MAX_COLUMNS;

/** The grade, speed and size columns: the same grid in heads and rows. */
export const columnsClass = (langs: string[]) =>
  gradeMode(langs) === "overall"
    ? "grid-cols-[44px_40px_52px] gap-x-[6px]"
    : isDense(langs)
      ? "grid-cols-[repeat(var(--n),26px)_36px_50px] gap-x-[3px]"
      : "grid-cols-[repeat(var(--n),32px)_40px_52px] gap-x-[6px]";

export const columnsStyle = (langs: string[]) =>
  ({ "--n": langs.length }) as React.CSSProperties;

/** The row's grid: mark, name, columns, the delete/stop slot. */
export const ROW_GRID =
  "grid grid-cols-[18px_minmax(0,1fr)_auto_24px] items-center gap-x-[10px]";

export const CODE_CLASS =
  "text-tag font-semibold leading-[14px] tracking-[0.02em] text-ink-3 uppercase";

/** The column codes (EN ES … SPEED SIZE), over the columns they name. */
export const Codes: React.FC<{ langs: string[] }> = ({ langs }) => {
  const { t, i18n } = useTranslation();
  return (
    <span
      style={columnsStyle(langs)}
      className={`grid items-center justify-items-center ${columnsClass(langs)}`}
    >
      {gradeMode(langs) === "each" ? (
        langs.map((l) => (
          <span
            key={l}
            title={languageName(l, i18n.language)}
            className={CODE_CLASS}
          >
            {l}
          </span>
        ))
      ) : (
        <span className={CODE_CLASS} title={t("onboarding.model.accuracy")}>
          {t("settings.models.allLanguages")}
        </span>
      )}
      <span className={CODE_CLASS}>{t("onboarding.model.speed")}</span>
      <span className={CODE_CLASS}>{t("onboarding.model.size")}</span>
    </span>
  );
};
