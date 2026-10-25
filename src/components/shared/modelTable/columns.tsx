import React from "react";
import { useTranslation } from "react-i18next";
import type { CatalogModel } from "@handy-computer/models";
import { Tooltip } from "@/components/ui/Tooltip";
import { GradeChip } from "@/components/shared/GradeChip";
import { SpeedBars, speedLevel } from "@/components/shared/SpeedBars";
import { useRubric } from "@/components/onboarding/Rubric";
import { languageName } from "@/lib/constants/languages";
import { formatModelSize } from "@/lib/utils/format";
import type { Grading } from "./grading";

/**
 * A grade for each language up to three; four and five take the dense
 * chips; past five the grades fold into one overall grade per row, as at
 * the Models page's width (about 540px) a sixth column leaves the name no
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

/** A row's grid: mark, name, columns, and the delete/stop slot if any. */
export const rowGrid = (actions: boolean) =>
  `grid ${actions ? "grid-cols-[18px_minmax(0,1fr)_auto_24px]" : "grid-cols-[18px_minmax(0,1fr)_auto]"} items-center gap-x-[10px]`;

export const CODE_CLASS =
  "text-tag font-semibold leading-[14px] tracking-[0.02em] text-ink-3 uppercase";

/** A part's view-transition name (onboarding's moves), or nothing. */
export type VtName = (part?: string) => Record<string, string> | undefined;

/** The column codes (EN ES … SPEED SIZE), over the columns they name. */
export const Codes: React.FC<{ langs: string[]; vt?: VtName }> = ({
  langs,
  vt,
}) => {
  const { t, i18n } = useTranslation();
  return (
    <span
      {...vt?.("c")}
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

/**
 * A grade chip's hint: what the letter means for these languages (one, or
 * all of yours with each one's grade under it), or why there's none.
 */
export const GradeTip: React.FC<{
  grading: Grading;
  model: CatalogModel | undefined;
  /** The languages the chip grades: one, or all of yours. */
  langs: string[];
  children: React.ReactElement;
}> = ({ grading, model, langs, children }) => {
  const { t, i18n } = useTranslation();
  const ui = i18n.language;
  const nameOf = (l: string) => languageName(l, ui);
  const list = (codes: string[]) =>
    new Intl.ListFormat(ui, { type: "conjunction" }).format(codes.map(nameOf));
  const one = langs.length === 1;
  const grade = one ? grading.grade(model, langs[0]) : grading.overall(model);
  const key = (k: string) => `onboarding.model.rubric.${k}`;
  const miss = langs.filter((l) => grading.grade(model, l).unsupported);
  let text: React.ReactNode;
  if (!model) text = t(key("unrated"), { language: list(langs) });
  else if (miss.length) text = t(key("unsupported"), { language: list(miss) });
  else if (grade.letter === "none")
    text = t(key("unrated"), { language: list(langs) });
  else text = t(key(`meaning.${grade.letter}`));
  // Several languages: each one's grade under the line, as words (the
  // chips' greys are for the page, not the tooltip's plum).
  const breakdown = !one && (
    <span className="mt-[6px] grid grid-cols-[1fr_auto] gap-x-[16px] tabular-nums">
      {langs.map((l) => (
        <React.Fragment key={l}>
          <span>{nameOf(l)}</span>
          <b className="text-end font-semibold">
            {grading.grade(model, l).label}
          </b>
        </React.Fragment>
      ))}
    </span>
  );
  return (
    <Tooltip
      title={
        one
          ? t(key(grade.letter !== "none" ? "gradeTitle" : "accuracyTitle"), {
              language: nameOf(langs[0]),
              grade: grade.label,
            })
          : t(key(grade.letter !== "none" ? "overallTitle" : "overall"), {
              grade: grade.label,
            })
      }
      text={
        <>
          {text}
          {breakdown}
        </>
      }
    >
      {children}
    </Tooltip>
  );
};

/** How fast, in words: a benchmark band, never measured seconds. */
export const useSpeedWords = () => {
  const { t } = useTranslation();
  return (wait: number | undefined, preview = false) =>
    wait == null
      ? t("onboarding.model.speedUntested")
      : t(
          `onboarding.model.${preview ? "speedWordsPreview" : "speedWords"}.${speedLevel(wait)}`,
        );
};

/**
 * The speed bars' hint: how fast in words, and the seconds its bars stand
 * for on this tier's test machine. `className` is the anchor's.
 */
export const SpeedTip: React.FC<{
  grading: Grading;
  model: CatalogModel | undefined;
  /** It previews your words while you talk. */
  streaming: boolean;
  className?: string;
  children: React.ReactElement;
}> = ({ grading, model, streaming, className, children }) => {
  const { t } = useTranslation();
  const rubric = useRubric();
  const words = useSpeedWords();
  const wait = grading.wait(model);
  return (
    <Tooltip
      className={className}
      title={words(wait, streaming)}
      text={
        wait == null
          ? t("onboarding.model.rubric.untimed")
          : t(`onboarding.model.rubric.speedText.${grading.tier}`, {
              band: rubric.speedBand(speedLevel(wait)),
            })
      }
    >
      {children}
    </Tooltip>
  );
};

/** A model's values under the codes: its grades, speed and size. */
export const Values: React.FC<{
  grading: Grading;
  model: CatalogModel | undefined;
  streaming: boolean;
  sizeMb: number;
  vt?: VtName;
}> = ({ grading, model, streaming, sizeMb, vt }) => {
  const { langs } = grading;
  return (
    <span
      {...vt?.("g")}
      style={columnsStyle(langs)}
      className={`grid items-center justify-items-center ${columnsClass(langs)}`}
    >
      {gradeMode(langs) === "each" ? (
        langs.map((l) => {
          const g = grading.grade(model, l);
          return (
            <GradeTip key={l} grading={grading} model={model} langs={[l]}>
              <span className="inline-grid">
                <GradeChip
                  letter={g.letter}
                  label={g.label}
                  dense={isDense(langs)}
                />
              </span>
            </GradeTip>
          );
        })
      ) : (
        <GradeTip grading={grading} model={model} langs={langs}>
          <span className="inline-grid">
            <GradeChip
              letter={grading.overall(model).letter}
              label={grading.overall(model).label}
            />
          </span>
        </GradeTip>
      )}
      <SpeedTip grading={grading} model={model} streaming={streaming}>
        <span className="inline-grid h-[22px] w-[36px] place-items-center">
          <SpeedBars
            level={speedLevel(grading.wait(model))}
            className="h-[12px]!"
          />
        </span>
      </SpeedTip>
      <span
        {...vt?.("s")}
        className="whitespace-nowrap text-label tabular-nums text-ink-3"
      >
        {sizeMb > 0 ? formatModelSize(sizeMb) : "—"}
      </span>
    </span>
  );
};
