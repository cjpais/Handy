import React, {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import { Languages, Radio as LiveIcon } from "lucide-react";
import { Button } from "../../ui/Button";
import { Radio } from "../../ui/Radio";
import { GradeChip } from "../../shared/GradeChip";
import { Hint } from "../../shared/Hint";
import { ProgressLine } from "../../shared/ProgressLine";
import { SpeedBars, speedLevel } from "../../shared/SpeedBars";
import { Tag } from "../../shared/Tag";
import { formatModelSize } from "@/lib/utils/format";
import { OnboardingShell, ScreenHeader, ScreenMain } from "../OnboardingShell";
import { useOnboardingStore } from "../onboardingStore";
import { useDownloadWords } from "../DownloadStatus";
import { useRubric } from "../Rubric";
import { languageName } from "@/lib/constants/languages";
import {
  errorFor,
  geoMean,
  gradeFor,
  gradeForError,
  type CatalogModel,
  type ModelRow,
  type PickRole,
} from "../data/models";
import type { useOnboardingModels } from "../useOnboardingModels";
import { moveModels } from "./modelTransitions";
import "./modelTransitions.css";

type Models = ReturnType<typeof useOnboardingModels>;

/** A card's words for each pick: the Full card's line and the short ones. */
const ROLE_KEYS: Record<PickRole | "extra", string> = {
  pick: "recommended",
  faster: "faster",
  accurate: "accurate",
  extra: "fromAll",
};

const Chevron: React.FC<{ dir?: "down" | "up" }> = ({ dir = "down" }) => (
  <span
    aria-hidden="true"
    className={`inline-block size-[6px] shrink-0 border-e-[1.5px] border-b-[1.5px] border-current ${
      dir === "down" ? "-mt-[3px] rotate-45" : "mt-[3px] -rotate-[135deg]"
    }`}
  />
);

const SearchIcon = () => (
  <svg
    width="15"
    height="15"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    aria-hidden="true"
  >
    <circle cx="11" cy="11" r="7" />
    <path d="M20 20l-3.5-3.5" />
  </svg>
);

/** The view-transition name of a model's part, the same in every state. */
const vt = (id: string, part = "") => ({
  "data-vt":
    "mf-" + id.toLowerCase().replace(/[^a-z0-9]+/g, "-") + (part && "-" + part),
});

interface ModelProps {
  progress?: { index: number; count: number };
  models: Models;
  onBack?: () => void;
  /** The picked model (catalog id), once Download or Continue is pressed. */
  onChoose: (modelId: string) => void;
}

/**
 * Screen 3: a speech model for your languages and this computer. Three picks
 * (Recommended, Faster, More accurate), the selected one as the Full card;
 * Show all opens every model in place as a table with search.
 */
export const Model: React.FC<ModelProps> = ({
  progress,
  models,
  onBack,
  onChoose,
}) => {
  const { t, i18n } = useTranslation();
  const words = useDownloadWords();
  const rubric = useRubric();
  const { languages, modelId, extraModelId, downloadId, set } =
    useOnboardingStore();
  const [showAll, setShowAll] = useState(false);
  const [query, setQuery] = useState("");
  // Show all's filters: only models that preview, only models that translate.
  const [onlyPreview, setOnlyPreview] = useState(false);
  const [onlyTranslate, setOnlyTranslate] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const focusSearch = useRef(false);

  const langs = languages.length ? languages : ["en"];
  const dense = langs.length > 5;
  // Both are shown whatever your settings: they're what the model can do,
  // the same as on the Models page, and the hints say where to see them. A
  // streaming model previews your words in the Live overlay while you talk.
  const previews = (m: CatalogModel) => m.capabilities.streaming;
  const translates = (m: CatalogModel) => m.capabilities.translate;
  const { picks, groups, offered } = models;
  const pick = picks.find((p) => p.role === "pick") ?? picks[0];
  const selectedId = modelId ?? downloadId ?? pick?.model.id ?? null;
  const isPick = (id: string | null) =>
    !!id && picks.some((p) => p.model.id === id);
  // A model picked from the full list stays on Model as the fourth item,
  // unless it's one of the picks now (new languages, or picked before).
  const extraId =
    selectedId && !isPick(selectedId)
      ? selectedId
      : isPick(extraModelId)
        ? null
        : extraModelId;
  useEffect(() => {
    if (extraId !== extraModelId) set({ extraModelId: extraId });
  }, [extraId, extraModelId, set]);

  const byId = useMemo(() => new Map(offered.map((m) => [m.id, m])), [offered]);
  const rowFor = (id: string): ModelRow | undefined =>
    [...groups.picks, ...groups.others, ...groups.partial].find(
      (r) => r.model.id === id,
    );

  const ui = i18n.language;
  const nameOf = (code: string) => languageName(code, ui);
  const sizeOf = (m: CatalogModel) =>
    formatModelSize(models.infoFor(m)?.size_mb);
  // In a column, an unknown size is a dash: "Unknown size" won't fit.
  const sizeCell = (m: CatalogModel) =>
    (models.infoFor(m)?.size_mb ?? 0) > 0 ? sizeOf(m) : "—";
  // Only a word, no seconds: the wait is a benchmark from machines like this
  // one, not measured on it, so a number would promise more than we know.
  // A model that previews says so after its speed.
  const speedText = (wait: number | undefined, preview = false) =>
    wait == null
      ? t("onboarding.model.speedUntested")
      : t(
          `onboarding.model.${preview ? "speedWordsPreview" : "speedWords"}.${speedLevel(wait)}`,
        );

  /** "English", "English and Spanish": names in a sentence. */
  const listOf = (codes: string[]) =>
    new Intl.ListFormat(ui, { type: "conjunction" }).format(codes.map(nameOf));

  /**
   * A grade chip's hint, as the capability tags have, the same for one
   * language's grade and for one grade over all of them: that it grades how
   * well the model hears that language, what the letter means for your
   * words, and the error it was measured at. Over several languages the
   * error is their average, as the chip's.
   */
  const gradeHint = (
    m: CatalogModel,
    ls: string[],
    chip: React.ReactElement,
  ) => {
    const one = ls.length === 1;
    const errors = ls.map((l) => errorFor(m, l));
    const error = errors.every((e) => e != null)
      ? one
        ? errors[0]
        : geoMean(errors as number[])
      : undefined;
    const grade = one
      ? gradeFor(m, ls[0])
      : error != null
        ? gradeForError(error)
        : undefined;
    const missing = ls.filter((l) => gradeFor(m, l).unsupported);
    const graded = grade && grade.letter !== "none" ? grade : undefined;
    const key = (k: string) => `onboarding.model.rubric.${k}`;
    return (
      <Hint
        key={ls.join()}
        title={
          one
            ? t(key(graded ? "gradeTitle" : "accuracyTitle"), {
                language: nameOf(ls[0]),
                grade: graded?.label,
              })
            : t(key(graded ? "overallTitle" : "overall"), {
                grade: graded?.label,
              })
        }
        text={
          missing.length
            ? t(key("unsupported"), {
                language: listOf(missing),
              })
            : error == null || !graded || graded.letter === "none"
              ? t(key("unrated"), {
                  language: listOf(ls.filter((l) => errorFor(m, l) == null)),
                })
              : `${t(key(`meaning.${graded.letter}`))} ${t(
                  key(one ? "measured" : "measuredAverage"),
                  {
                    error: rubric.number(error),
                    letter: graded.letter,
                    band: rubric.gradeBand(graded.letter),
                  },
                )}`
        }
      >
        {chip}
      </Hint>
    );
  };

  /**
   * The speed bars' hint, the same wherever they are: how fast in words,
   * what was timed on which test machine, and the seconds its bars stand
   * for. `className` is the anchor's, the grid item.
   */
  const speedHint = (
    row: ModelRow,
    bars: React.ReactElement,
    className = "",
  ) => (
    <Hint
      className={className}
      title={speedText(row.wait, previews(row.model))}
      text={
        row.wait == null
          ? t("onboarding.model.rubric.untimed")
          : t(`onboarding.model.rubric.speedText.${models.tier}`, {
              band: rubric.speedBand(speedLevel(row.wait)),
            })
      }
    >
      {bars}
    </Hint>
  );

  /**
   * What a model can do besides transcribe: its word, icon and the line its
   * hint says. The same icon marks the rows, their tags and Show all's filter.
   */
  const CAPABILITIES = {
    preview: { Icon: LiveIcon, has: previews },
    translate: { Icon: Languages, has: translates },
  } as const;
  type Capability = keyof typeof CAPABILITIES;
  const capabilityName = (c: Capability) => t(`onboarding.model.tags.${c}`);
  const capabilityLine = (c: Capability) =>
    t(`onboarding.model.tags.${c}Title`);

  /** A capability's tag on a card: its icon and word. */
  const capabilityTag = (m: CatalogModel, c: Capability) => {
    const { Icon, has } = CAPABILITIES[c];
    return (
      has(m) && (
        <Hint text={capabilityLine(c)}>
          <Tag className="inline-flex items-center gap-[4px]">
            <Icon
              aria-hidden="true"
              className="size-[11px]"
              strokeWidth={2.25}
            />
            {capabilityName(c)}
          </Tag>
        </Hint>
      )
    );
  };
  const previewTag = (m: CatalogModel) => capabilityTag(m, "preview");
  const translateTag = (m: CatalogModel) => capabilityTag(m, "translate");

  /** Show all's version: the icons alone after the role, words on hover. */
  const capabilityIcons = (m: CatalogModel) =>
    (previews(m) || translates(m)) && (
      <span className="inline-flex shrink-0 items-center gap-[2px] text-ink-3">
        {(Object.keys(CAPABILITIES) as Capability[]).map((c) => {
          const { Icon, has } = CAPABILITIES[c];
          return (
            has(m) && (
              <Hint key={c} title={capabilityName(c)} text={capabilityLine(c)}>
                <span className="grid size-[20px] place-items-center hover:text-text">
                  <Icon
                    role="img"
                    aria-label={capabilityName(c)}
                    className="size-[14px]"
                  />
                </span>
              </Hint>
            )
          );
        })}
      </span>
    );

  /** What a model can do besides transcribe, beside its role. */
  const capabilityTags = (m: CatalogModel) => (
    <>
      {previewTag(m)}
      {translateTag(m)}
    </>
  );

  const move = (kind: "grow" | "shrink" | "pick", update: () => void) =>
    moveModels(rootRef.current, kind, update);

  const open = (initialQuery = "") =>
    move("grow", () => {
      focusSearch.current = true;
      setQuery(initialQuery);
      setShowAll(true);
    });
  const close = () =>
    move("shrink", () => {
      setShowAll(false);
      setQuery("");
      setOnlyPreview(false);
      setOnlyTranslate(false);
    });

  const choose = (id: string, animate: boolean) => {
    const update = () => {
      set({
        modelId: id,
        ...(isPick(id) ? {} : { extraModelId: id }),
      });
    };
    if (animate) move("pick", update);
    else update();
  };

  // Typing on Model opens every model with what's typed in the search.
  useEffect(() => {
    if (showAll) return;
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      if (target && /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName)) return;
      if (e.key.length !== 1 || e.key === " ") return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      e.preventDefault();
      open(e.key);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
    // open() is stable enough for a listener re-added with showAll
  }, [showAll]);

  // In the full list: Escape clears the search, then folds the list; typing
  // anywhere goes into the search.
  useEffect(() => {
    if (!showAll) return;
    if (focusSearch.current) {
      focusSearch.current = false;
      const input = searchRef.current;
      input?.focus({ preventScroll: true });
      input?.setSelectionRange(input.value.length, input.value.length);
    }
    const onKey = (e: KeyboardEvent) => {
      const input = searchRef.current;
      if (e.key === "Escape") {
        e.preventDefault();
        if (input?.value) setQuery("");
        else close();
        return;
      }
      // Space stays with what has focus: it presses a filter or a row.
      if (
        input &&
        document.activeElement !== input &&
        e.key.length === 1 &&
        e.key !== " " &&
        !e.metaKey &&
        !e.ctrlKey &&
        !e.altKey
      )
        input.focus({ preventScroll: true });
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [showAll]);

  // The full list fades out at the bottom while there's more below, as
  // Welcome's languages do; scrolled to the end, the last row shows whole.
  // Checked on every render (a search or filter changes what's below) and
  // on scroll.
  useLayoutEffect(() => {
    const list = listRef.current;
    if (!list) return;
    const edge = () =>
      list.toggleAttribute(
        "data-more",
        list.scrollTop + list.clientHeight < list.scrollHeight - 1,
      );
    edge();
    list.addEventListener("scroll", edge, { passive: true });
    return () => list.removeEventListener("scroll", edge);
  });

  // Coming into the full list: the selected row in view, once the list has
  // laid out (it can take a frame or two to get its height).
  useEffect(() => {
    if (!showAll) return;
    let id = 0;
    let tries = 0;
    const reveal = () => {
      const list = listRef.current;
      if (!list) return;
      if (!list.clientHeight && tries++ < 10) {
        id = requestAnimationFrame(reveal);
        return;
      }
      const row = list.querySelector<HTMLElement>("[data-vt-row].is-on");
      if (!row) return;
      const bottom = row.offsetTop + row.offsetHeight + 12 - list.clientHeight;
      if (list.scrollTop < bottom) list.scrollTop = bottom;
    };
    id = requestAnimationFrame(reveal);
    return () => cancelAnimationFrame(id);
  }, [showAll]);

  if (!pick || !selectedId) return null;
  const selected = byId.get(selectedId) ?? pick.model;

  /* ---------------- the download ---------------- */

  // The model downloading carries the Models page's line on its card,
  // wherever that card is: selected, small, or in Show all. One already on
  // disk has nothing to show; one that stopped says so on its full card.
  const loading = downloadId ? byId.get(downloadId) : undefined;
  const loadingState = models.downloadState(loading);
  const lineFor = (id: string) =>
    id === downloadId &&
    (loadingState.state === "downloading" || loadingState.state === "checking");
  /** The full card says so, stopped included. */
  const showsDownload = (id: string) =>
    lineFor(id) || (id === downloadId && loadingState.state === "failed");
  const lineValue =
    loadingState.state === "downloading" ? loadingState.progress : 1;
  /** A small card's line: 3px along its bottom, inside its padding. */
  const thinLine = (place: string) => (
    <ProgressLine
      value={lineValue}
      busy={loadingState.state === "checking"}
      className={`absolute h-[3px] ${place}`}
    />
  );

  /** The full card's line, with how far on the left and the speed on the right. */
  const fullLine = () => {
    const s = loadingState;
    if (s.state === "failed")
      return (
        <span className="mt-[10px] text-label text-warning">
          {t("onboarding.download.failed")}
        </span>
      );
    if (s.state !== "downloading" && s.state !== "checking") return null;
    let left: string;
    let right: string | null = null;
    if (s.state === "downloading") {
      const { done, total } = words.sizes(
        s,
        loading && models.infoFor(loading)?.size_mb,
      );
      const speed = words.speed(s);
      const time = words.time(s);
      left = t("onboarding.download.cardProgress", {
        percent: words.percent(s),
        done,
        total,
      });
      right =
        speed && time
          ? t("onboarding.download.speedLeft", { speed, time })
          : (speed ?? (time && t("onboarding.download.timeLeft", { time })));
    } else left = t("onboarding.download.checking");
    return (
      <span className="mt-[10px] flex flex-col gap-[5px] self-stretch">
        <ProgressLine value={lineValue} busy={s.state === "checking"} />
        <span className="flex justify-between gap-[12px] text-label tabular-nums text-ink-3">
          <span className="truncate">{left}</span>
          {right && <span className="shrink-0">{right}</span>}
        </span>
      </span>
    );
  };

  /* ---------------- the pieces ---------------- */

  const mark = (id: string, on: boolean) => (
    <span {...vt(id, "m")}>
      <Radio checked={on} />
    </span>
  );

  const roleBase = "shrink-0 font-[650] uppercase tracking-[0.04em]";
  const roleClass = `${roleBase} text-label text-ink-2`;
  const sizeClass =
    "ms-auto min-w-[50px] text-end text-label text-ink-3 tabular-nums";

  /**
   * The selected model: role and capability tags (as on the small cards) over
   * the name, its line, Accuracy and Speed.
   */
  const fullCard = (row: ModelRow, role: PickRole | "extra") => {
    const id = row.model.id;
    return (
      <div
        key={id}
        {...vt(id)}
        data-vt-row=""
        role="radio"
        aria-checked="true"
        tabIndex={0}
        className="flex w-full min-w-0 flex-col items-stretch rounded-panel border-[1.5px] border-plum-pick bg-background px-5 py-4 text-start text-small text-text [view-transition-class:mf-row_mf-box]"
      >
        <span className="flex min-w-0 items-center gap-[10px] whitespace-nowrap">
          {mark(id, true)}
          <b {...vt(id, "r")} className={`${roleBase} text-[16px] text-text`}>
            {t(`onboarding.model.roles.${ROLE_KEYS[role]}`)}
          </b>
          {capabilityTags(row.model)}
          <span {...vt(id, "s")} className={sizeClass}>
            {sizeOf(row.model)}
          </span>
        </span>
        <span className="flex flex-col items-start">
          <span
            {...vt(id, "n")}
            className="mt-[4px] font-display text-name font-[650] leading-[1.2] tracking-[-0.01em] text-ink-2"
          >
            {row.model.name}
          </span>
          {/* Many languages take two rows of grades: a download has no
            room left for the line, so it takes this one's. */}
          {!(dense && showsDownload(id)) && (
            <span {...vt(id, "d")} className="mt-[2px] text-ink-2">
              {role === "extra"
                ? row.model.description
                : t(`onboarding.model.lines.${ROLE_KEYS[role]}`)}
            </span>
          )}
          <span className="mt-[10px] grid grid-cols-[84px_1fr] items-center gap-x-[12px] gap-y-[8px] self-stretch">
            <span className="text-ink-2">{t("onboarding.model.accuracy")}</span>
            <span
              className={`grid gap-x-[18px] gap-y-[5px] ${dense ? "grid-cols-[repeat(4,max-content)]" : "grid-cols-[repeat(3,max-content)]"}`}
            >
              {langs.map((l) => {
                const grade = gradeFor(row.model, l);
                return gradeHint(
                  row.model,
                  [l],
                  <span className="inline-flex items-center gap-[6px]">
                    <GradeChip letter={grade.letter} label={grade.label} />
                    <small className="text-small">
                      {dense ? l.toUpperCase() : nameOf(l)}
                    </small>
                  </span>,
                );
              })}
            </span>
            <span className="text-ink-2">{t("onboarding.model.speed")}</span>
            {/* The words are beside the bars already: the hint's title
                names the row. */}
            {speedHint(
              row,
              <span className="flex flex-wrap items-center gap-x-[12px] gap-y-[5px]">
                <SpeedBars level={speedLevel(row.wait)} />
                {/* Plain: the Live preview tag on line 1 says it previews. */}
                {speedText(row.wait)}
              </span>,
              // Over the bars and words, not the whole column.
              "justify-self-start",
            )}
          </span>
        </span>
        {showsDownload(id) && fullLine()}
      </div>
    );
  };

  /**
   * The column heads (EN … SPEED SIZE) over the values, one grid each. Many
   * languages narrow the columns; the height stays the same.
   */
  const columns = (row: ModelRow) => {
    const cols = columnsClass(dense);
    const style = { "--n": langs.length } as React.CSSProperties;
    const id = row.model.id;
    return (
      <span className="flex flex-col gap-[5px]">
        <span
          {...vt(id, "c")}
          style={style}
          className={`grid items-center justify-items-center uppercase ${cols}`}
        >
          {langs.map((l) => (
            <span key={l} title={nameOf(l)} className={CODE_CLASS}>
              {l.toUpperCase()}
            </span>
          ))}
          <span className={CODE_CLASS}>{t("onboarding.model.speed")}</span>
          <span className={CODE_CLASS}>{t("onboarding.model.size")}</span>
        </span>
        <span
          {...vt(id, "g")}
          style={style}
          className={`grid items-center justify-items-center ${cols}`}
        >
          {langs.map((l) => {
            const grade = gradeFor(row.model, l);
            return gradeHint(
              row.model,
              [l],
              <span className="inline-grid">
                <GradeChip
                  letter={grade.letter}
                  label={grade.label}
                  dense={dense}
                />
              </span>,
            );
          })}
          {speedHint(
            row,
            <span
              className={`inline-grid h-[22px] place-items-center ${dense ? "w-[36px]" : "w-[40px]"}`}
            >
              <SpeedBars level={speedLevel(row.wait)} className="h-[12px]!" />
            </span>,
          )}
          <span
            {...vt(id, "s")}
            className="whitespace-nowrap text-label text-ink-3 tabular-nums"
          >
            {sizeCell(row.model)}
          </span>
        </span>
      </span>
    );
  };

  /** A pick that isn't selected: role, name and few words, then columns. */
  const card = (row: ModelRow, role: PickRole) => {
    const id = row.model.id;
    const dim = row.missing.length > 0;
    const line = lineFor(id);
    return (
      <button
        key={id}
        type="button"
        role="radio"
        aria-checked="false"
        {...vt(id)}
        data-vt-row=""
        onClick={() => choose(id, true)}
        className={`relative grid w-full min-w-0 cursor-pointer grid-cols-[18px_minmax(0,1fr)_auto] items-center gap-x-[10px] rounded-card border-[1.5px] border-line bg-background px-5 text-start text-small leading-[1.5] text-text hover:border-ink-3 hover:bg-wash [view-transition-class:mf-row_mf-box] ${line ? "pt-[8px] pb-[15px]" : "py-[8px]"}`}
      >
        {mark(id, false)}
        <span className="flex min-w-0 flex-col items-start">
          <span className="flex min-w-0 max-w-full items-baseline gap-[8px] whitespace-nowrap">
            <b {...vt(id, "r")} className={`${roleBase} text-label text-text`}>
              {t(`onboarding.model.roles.${ROLE_KEYS[role]}`)}
            </b>
            {capabilityTags(row.model)}
          </span>
          <span className="flex min-w-0 max-w-full items-baseline gap-[8px] whitespace-nowrap">
            <span
              {...vt(id, "n")}
              className={`min-w-0 shrink overflow-hidden text-ellipsis text-small font-semibold text-ink-2 ${dim ? "opacity-60" : ""}`}
            >
              {row.model.name}
            </span>
            {langs.length <= 3 && (
              <span
                {...vt(id, "d")}
                className={`min-w-0 flex-[1_1_0] overflow-hidden text-ellipsis text-ink-3 before:me-[8px] before:content-['·'] ${dim ? "opacity-60" : ""}`}
              >
                {t(`onboarding.model.shorts.${ROLE_KEYS[role]}`)}
              </span>
            )}
          </span>
        </span>
        {columns(row)}
        {line && thinLine("inset-x-5 bottom-[6px]")}
      </button>
    );
  };

  /** One line: role, few words (or the name), one grade, speed and size. */
  const oneLine = (row: ModelRow, role: PickRole | "extra") => {
    const id = row.model.id;
    const line = lineFor(id);
    const overall =
      row.error != null
        ? gradeForError(row.error)
        : row.missing.length
          ? ({ letter: "none", label: "—" } as const)
          : ({ letter: "none", label: "?" } as const);
    return (
      <button
        key={id}
        type="button"
        role="radio"
        aria-checked="false"
        {...vt(id)}
        data-vt-row=""
        onClick={() => choose(id, true)}
        className={`relative flex w-full min-w-0 cursor-pointer items-center gap-[10px] whitespace-nowrap rounded-card border-[1.5px] border-line bg-background px-5 text-start text-small text-text hover:border-ink-3 hover:bg-wash [view-transition-class:mf-row_mf-box] ${line ? "pt-[8px] pb-[15px]" : "py-[8px]"}`}
      >
        {mark(id, false)}
        <b {...vt(id, "r")} className={`${roleClass} text-text`}>
          {t(`onboarding.model.roles.${ROLE_KEYS[role]}`)}
        </b>
        {capabilityTags(row.model)}
        <span
          {...vt(id, "d")}
          className={`min-w-0 flex-[1_1_0] overflow-hidden text-ellipsis text-ink-3 ${row.missing.length ? "opacity-60" : ""}`}
        >
          {role === "extra"
            ? row.model.name
            : t(`onboarding.model.shorts.${ROLE_KEYS[role]}`)}
        </span>
        <span className="ms-auto flex shrink-0 items-center gap-[10px]">
          <span {...vt(id, "g")} className="flex items-center gap-[10px]">
            {gradeHint(
              row.model,
              langs,
              <span className="inline-grid">
                <GradeChip letter={overall.letter} label={overall.label} />
              </span>,
            )}
            {speedHint(
              row,
              <span className="inline-grid">
                <SpeedBars level={speedLevel(row.wait)} />
              </span>,
            )}
          </span>
          <span {...vt(id, "s")} className={`${sizeClass} ms-0`}>
            {sizeOf(row.model)}
          </span>
        </span>
        {line && thinLine("inset-x-5 bottom-[6px]")}
      </button>
    );
  };

  /* ---------------- the footer ---------------- */

  // Nothing to download for a model on disk, or the one already downloading.
  // Its download stopped: the button downloads it again.
  const selectedState = models.downloadState(selected).state;
  const continuing =
    selectedState === "ready" ||
    (downloadId === selectedId && selectedState !== "failed");
  // Another model picked mid-download: what the switch costs, where it's
  // made. The download pauses (its part stays on disk), so no dialog.
  const switching =
    loading && loading.id !== selectedId && loadingState.state === "downloading"
      ? { name: loading.name, percent: words.percent(loadingState) }
      : null;
  const primary = (
    <Button variant="primary" size="lg" onClick={() => onChoose(selectedId)}>
      {continuing
        ? t("onboarding.shell.continue")
        : t(
            switching
              ? "onboarding.model.downloadInstead"
              : "onboarding.model.download",
            { size: sizeOf(selected) },
          )}
    </Button>
  );
  const hint = switching ? (
    <div className="text-start leading-[1.35]">
      <div className="truncate font-medium text-text">
        {t("onboarding.model.pauses", switching)}
      </div>
      <div className="truncate text-label">
        {t("onboarding.model.resumeLater")}
      </div>
    </div>
  ) : continuing ? (
    ""
  ) : (
    t("onboarding.model.hint")
  );

  const title = <span data-vt="mf-title">{t("onboarding.model.title")}</span>;

  /* ---------------- the picks ---------------- */

  if (!showAll) {
    const extra = extraId ? rowFor(extraId) : undefined;
    const items = picks.map((p) => {
      const row = rowFor(p.model.id);
      if (!row) return null;
      if (p.model.id === selectedId) return fullCard(row, p.role);
      return extra ? oneLine(row, p.role) : card(row, p.role);
    });
    if (extra)
      items.push(
        extra.model.id === selectedId
          ? fullCard(extra, "extra")
          : oneLine(extra, "extra"),
      );
    return (
      <OnboardingShell
        progress={progress}
        onBack={onBack}
        hint={hint}
        primary={primary}
      >
        <ScreenMain
          ref={rootRef}
          data-model-root=""
          align="stretch"
          header={
            <ScreenHeader title={title} sub={t("onboarding.model.subtitle")} />
          }
        >
          <div
            role="radiogroup"
            aria-label={t("onboarding.model.label")}
            className="flex w-full flex-none flex-col gap-[8px] text-start"
          >
            {items}
          </div>
          <button
            type="button"
            onClick={() => open()}
            className="mt-[8px] flex h-[38px] flex-none cursor-pointer items-center gap-[8px] self-stretch rounded-card border-[1.5px] border-dashed border-line-strong px-[20px] text-start text-small font-semibold text-ink-2 hover:border-solid hover:bg-wash hover:text-text"
          >
            {t("onboarding.model.showAll", { count: models.count })}
            <Chevron />
          </button>
        </ScreenMain>
      </OnboardingShell>
    );
  }

  /* ---------------- every model ---------------- */

  const q = query.trim().toLowerCase();
  const matches = (row: ModelRow) =>
    !q ||
    q
      .split(/\s+/)
      .every((w) =>
        [
          row.model.name,
          row.model.description,
          previews(row.model) ? t("onboarding.model.tags.preview") : "",
          translates(row.model) ? t("onboarding.model.tags.translate") : "",
          ...row.model.languages.flatMap((l) => [
            languageName(l, ui),
            languageName(l, "en"),
          ]),
        ]
          .join(" ")
          .toLowerCase()
          .includes(w),
      );
  const kept = (row: ModelRow) =>
    matches(row) &&
    (!onlyPreview || previews(row.model)) &&
    (!onlyTranslate || translates(row.model));
  const picked = groups.picks.filter(kept);
  const others = groups.others.filter(kept);
  const partial = groups.partial.filter(kept);
  // The preview filter only when some model can preview.
  const canPreview = offered.some(previews);

  /**
   * A filter in the search field, after a divider: the rows' icon and word at
   * the field's own size, pressed to keep only the models that have it.
   * Quieter than the search, which is what's mostly used; pressed fills grey.
   */
  const filter = (c: Capability, on: boolean, toggle: () => void) => {
    const { Icon } = CAPABILITIES[c];
    return (
      <Hint text={capabilityLine(c)}>
        <button
          type="button"
          aria-pressed={on}
          onClick={() => {
            toggle();
            if (listRef.current) listRef.current.scrollTop = 0;
          }}
          className={`flex h-[26px] shrink-0 cursor-pointer items-center gap-[6px] whitespace-nowrap rounded-[7px] px-[8px] text-small font-medium transition-colors focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary ${
            on
              ? "bg-line text-text"
              : "text-ink-3 hover:bg-wash hover:text-text"
          }`}
        >
          <Icon aria-hidden="true" className="size-[14px]" />
          {capabilityName(c)}
        </button>
      </Hint>
    );
  };
  const select = (id: string) => choose(id, false);

  /** ↑↓ from the search box move the selection. */
  const step = (d: number) => {
    const rows = [
      ...(listRef.current?.querySelectorAll<HTMLElement>("[data-vt-row]") ??
        []),
    ];
    if (!rows.length) return;
    const i = rows.findIndex((el) => el.classList.contains("is-on"));
    const next =
      rows[Math.max(0, Math.min(rows.length - 1, i < 0 ? 0 : i + d))];
    next.click();
    requestAnimationFrame(() =>
      listRef.current
        ?.querySelector(".is-on")
        ?.scrollIntoView({ block: "nearest" }),
    );
  };

  const chipCols = columnsClass(dense);
  const chipStyle = { "--n": langs.length } as React.CSSProperties;

  const tableRow = (row: ModelRow) => {
    const id = row.model.id;
    const on = id === selectedId;
    const dim = row.missing.length > 0;
    const line = lineFor(id);
    return (
      <button
        key={id}
        type="button"
        role="radio"
        aria-checked={on}
        {...vt(id)}
        data-vt-row=""
        onClick={() => select(id)}
        className={`relative grid w-full min-w-0 flex-none cursor-pointer grid-cols-[18px_minmax(0,1fr)_auto] items-center gap-x-[10px] rounded-control px-[14px] text-start text-small leading-[1.5] text-text [view-transition-class:mf-row_mf-box] ${line ? "pt-[9px] pb-[16px]" : "py-[9px]"} ${
          // Picked: the picks' plum edge (the Full card's), drawn inside so
          // the columns stay under their heads.
          on
            ? "is-on shadow-[inset_0_0_0_1.5px_var(--color-plum-pick)]"
            : "hover:bg-wash"
        }`}
      >
        {mark(id, on)}
        <span className="flex min-w-[150px] flex-col items-start">
          <span className="flex min-w-0 max-w-full items-center gap-[8px]">
            <b
              {...vt(id, "n")}
              className={`min-w-0 overflow-hidden text-ellipsis whitespace-nowrap font-[650] ${dim ? "opacity-60" : ""}`}
            >
              {row.model.name}
            </b>
            {row.role && (
              <Tag {...vt(id, "r")} on={on}>
                {t(`onboarding.model.roles.${ROLE_KEYS[row.role]}`)}
              </Tag>
            )}
            {capabilityIcons(row.model)}
          </span>
          <span
            {...vt(id, "d")}
            title={row.model.description}
            className={`max-w-full overflow-hidden text-ellipsis whitespace-nowrap text-ink-3 ${dim ? "opacity-60" : ""}`}
          >
            {row.model.description}
          </span>
        </span>
        <span
          {...vt(id, "g")}
          style={chipStyle}
          className={`grid items-center justify-items-center ${chipCols}`}
        >
          {langs.map((l) => {
            const grade = gradeFor(row.model, l);
            return gradeHint(
              row.model,
              [l],
              <span className="inline-grid">
                <GradeChip
                  letter={grade.letter}
                  label={grade.label}
                  dense={dense}
                />
              </span>,
            );
          })}
          {speedHint(
            row,
            <span
              className={`inline-grid h-[22px] place-items-center ${dense ? "w-[36px]" : "w-[40px]"}`}
            >
              <SpeedBars level={speedLevel(row.wait)} className="h-[12px]!" />
            </span>,
          )}
          <span
            {...vt(id, "s")}
            className="whitespace-nowrap text-label text-ink-3 tabular-nums"
          >
            {sizeCell(row.model)}
          </span>
        </span>
        {line && thinLine("inset-x-[14px] bottom-[7px]")}
      </button>
    );
  };

  const empty = !picked.length && !others.length && !partial.length;

  return (
    <OnboardingShell
      progress={progress}
      onBack={close}
      hint={hint}
      primary={primary}
    >
      <ScreenMain
        ref={rootRef}
        data-model-root=""
        layout="top"
        wide
        align="stretch"
        header={<ScreenHeader title={title} />}
      >
        <div
          data-vt="mf-tools"
          className="flex items-center gap-[10px] text-start"
        >
          {/* One field: the search, then its filters after a divider. */}
          <div className="flex h-[34px] min-w-0 flex-1 items-center gap-[4px] rounded-lg border border-line-strong bg-background pe-[4px]">
            <label className="flex h-full min-w-0 flex-1 cursor-text items-center gap-[7px] ps-[11px]">
              <span className="grid shrink-0 text-ink-3">
                <SearchIcon />
              </span>
              {/* Quiet on focus: the caret says where typing goes, and the
                  picked row below stays the one marked thing. No ×:
                  Escape clears. */}
              <input
                ref={searchRef}
                type="search"
                value={query}
                placeholder={t("onboarding.model.searchPlaceholder")}
                aria-label={t("onboarding.model.searchLabel")}
                onChange={(e) => {
                  setQuery(e.target.value);
                  if (listRef.current) listRef.current.scrollTop = 0;
                }}
                onKeyDown={(e) => {
                  const d =
                    e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
                  if (d) {
                    e.preventDefault();
                    step(d);
                  }
                }}
                className="h-full min-w-0 flex-1 bg-transparent text-small text-text outline-none placeholder:text-ink-3 [&::-webkit-search-cancel-button]:appearance-none"
              />
            </label>
            <span
              aria-hidden="true"
              className="me-[2px] h-[16px] w-px shrink-0 bg-line"
            />
            {canPreview &&
              filter("preview", onlyPreview, () => setOnlyPreview((v) => !v))}
            {filter("translate", onlyTranslate, () =>
              setOnlyTranslate((v) => !v),
            )}
          </div>
        </div>
        {!empty && (
          // The column heads sit above the list, not in it, so nothing
          // scrolls behind or past them: rows are cut off at the rule. The
          // same 4px bleed and scrollbar gutter as the list keep the
          // columns over theirs.
          <div className="-mx-[4px] mt-3 flex-none overflow-y-hidden px-[4px] [scrollbar-gutter:stable] [scrollbar-width:thin]">
            <div className="flex h-[31px] items-center justify-between border-b border-line ps-[42px] pe-[14px] uppercase">
              <span className={CODE_CLASS}>{t("onboarding.model.model")}</span>
              <span
                style={chipStyle}
                className={`grid items-center justify-items-center ${chipCols}`}
              >
                {langs.map((l) => (
                  <span key={l} title={nameOf(l)} className={CODE_CLASS}>
                    {l.toUpperCase()}
                  </span>
                ))}
                <span className={CODE_CLASS}>
                  {t("onboarding.model.speed")}
                </span>
                <span className={CODE_CLASS}>{t("onboarding.model.size")}</span>
              </span>
            </div>
          </div>
        )}
        <div
          ref={listRef}
          role="radiogroup"
          aria-label={t("onboarding.model.allLabel")}
          className="model-flow-scroll relative -mx-[4px] flex min-h-0 flex-1 flex-col gap-[4px] overflow-y-auto scroll-pt-[12px] px-[4px] pt-[12px] pb-[12px] text-start [scrollbar-width:thin] [scrollbar-gutter:stable] data-more:[mask-image:linear-gradient(to_bottom,black_calc(100%-40px),transparent_calc(100%-6px))]"
        >
          {picked.map(tableRow)}
          {others.map(tableRow)}
          {/* The models that miss one of their languages come last, under
              a note that says so; their rows are dimmed and grade "—". */}
          {partial.length > 0 && (
            <div className="mt-[6px] flex h-[34px] flex-none items-center border-t border-line px-[2px] text-small text-ink-3">
              {t("onboarding.model.missingLanguages")}
            </div>
          )}
          {partial.map(tableRow)}
          {empty && (
            <div className="px-[12px] py-[28px] text-center text-small text-ink-2">
              {query.trim()
                ? t("onboarding.model.noMatch", { query: query.trim() })
                : t("onboarding.model.noFilterMatch")}
            </div>
          )}
        </div>
      </ScreenMain>
    </OnboardingShell>
  );
};

/**
 * The grade, speed and size columns, the same in the pick cards and the
 * table. Every column centers its head over its values; Size is as wide as
 * its widest value ("99.0 MB").
 */
const columnsClass = (dense: boolean) =>
  dense
    ? "grid-cols-[repeat(var(--n),26px)_36px_48px] gap-x-[3px]"
    : "grid-cols-[repeat(var(--n),32px)_40px_48px] gap-x-[6px]";

const CODE_CLASS =
  "text-tag font-semibold leading-[14px] tracking-[0.02em] text-ink-3";
