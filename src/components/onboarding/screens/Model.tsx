import React, { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../ui/Button";
import { Radio } from "../../ui/Radio";
import { GradeChip } from "../../shared/GradeChip";
import { SpeedBars, speedLevel } from "../../shared/SpeedBars";
import { Tag } from "../../shared/Tag";
import { formatModelSize } from "@/lib/utils/format";
import {
  OnboardingShell,
  ScreenGap,
  ScreenMain,
  ScreenSub,
  ScreenTitle,
} from "../OnboardingShell";
import { useOnboardingStore } from "../onboardingStore";
import { languageName } from "../data/languages";
import {
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
  pill?: React.ReactNode;
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
  pill,
  onBack,
  onChoose,
}) => {
  const { t, i18n } = useTranslation();
  const { languages, modelId, extraModelId, downloadId, set } =
    useOnboardingStore();
  const [showAll, setShowAll] = useState(false);
  const [query, setQuery] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const focusSearch = useRef(false);

  const langs = languages.length ? languages : ["en"];
  const dense = langs.length > 5;
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
  const seconds = (s: number) =>
    t("onboarding.model.seconds", {
      value: new Intl.NumberFormat(ui, {
        maximumFractionDigits: s < 10 ? 1 : 0,
        minimumFractionDigits: s < 10 ? 1 : 0,
      }).format(s),
    });
  const speedText = (wait: number | undefined) =>
    wait == null
      ? t("onboarding.model.speedUntested")
      : t("onboarding.model.speedLine", {
          speed: t(`onboarding.model.speedWords.${speedLevel(wait)}`),
          wait: seconds(wait),
        });

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
      if (
        input &&
        document.activeElement !== input &&
        e.key.length === 1 &&
        !e.metaKey &&
        !e.ctrlKey &&
        !e.altKey
      )
        input.focus({ preventScroll: true });
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [showAll]);

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

  /* ---------------- the pieces ---------------- */

  const mark = (id: string, on: boolean) => (
    <span {...vt(id, "m")}>
      <Radio checked={on} />
    </span>
  );

  const roleClass =
    "shrink-0 text-label font-[650] uppercase tracking-[0.04em] text-ink-2";
  const sizeClass =
    "ms-auto min-w-[50px] text-end text-label text-ink-3 tabular-nums";

  /** The selected model: role over the name, its line, Accuracy and Speed. */
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
        className="flex w-full min-w-0 flex-col items-stretch rounded-panel border-[1.5px] border-plum/60 bg-background px-5 py-4 text-start text-small text-text [view-transition-class:mf-row_mf-box]"
      >
        <span className="flex min-w-0 items-center gap-[10px] whitespace-nowrap">
          {mark(id, true)}
          <b {...vt(id, "r")} className={roleClass}>
            {t(`onboarding.model.roles.${ROLE_KEYS[role]}`)}
          </b>
          <span {...vt(id, "s")} className={sizeClass}>
            {sizeOf(row.model)}
          </span>
        </span>
        <span className="flex flex-col items-start">
          <span
            {...vt(id, "n")}
            className="mt-[4px] font-display text-name font-[650] leading-[1.2] tracking-[-0.01em]"
          >
            {row.model.name}
          </span>
          <span {...vt(id, "d")} className="mt-[2px] text-ink-2">
            {role === "extra"
              ? row.model.description
              : t(`onboarding.model.lines.${ROLE_KEYS[role]}`)}
          </span>
          <span className="mt-[10px] grid grid-cols-[84px_1fr] items-center gap-x-[12px] gap-y-[8px] self-stretch">
            <span className="text-ink-3">{t("onboarding.model.accuracy")}</span>
            <span
              className={`grid gap-x-[18px] gap-y-[5px] ${dense ? "grid-cols-[repeat(4,max-content)]" : "grid-cols-[repeat(3,max-content)]"}`}
            >
              {langs.map((l) => {
                const grade = gradeFor(row.model, l);
                return (
                  <span
                    key={l}
                    title={nameOf(l)}
                    className="inline-flex items-center gap-[6px]"
                  >
                    <GradeChip letter={grade.letter} label={grade.label} />
                    <small className="text-small">
                      {dense ? l.toUpperCase() : nameOf(l)}
                    </small>
                  </span>
                );
              })}
            </span>
            <span className="text-ink-3">{t("onboarding.model.speed")}</span>
            <span className="flex flex-wrap items-center gap-x-[12px] gap-y-[5px]">
              <SpeedBars level={speedLevel(row.wait)} />
              {speedText(row.wait)}
            </span>
          </span>
        </span>
      </div>
    );
  };

  /**
   * The column heads (EN … SPEED SIZE) over the values, one grid each. Many
   * languages narrow the columns; the height stays the same.
   */
  const columns = (row: ModelRow) => {
    const cols = dense
      ? "grid-cols-[repeat(var(--n),26px)_36px_44px] gap-x-[3px]"
      : "grid-cols-[repeat(var(--n),32px)_40px_44px] gap-x-[6px]";
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
            return (
              <GradeChip
                key={l}
                letter={grade.letter}
                label={grade.label}
                dense={dense}
              />
            );
          })}
          <span
            title={speedText(row.wait)}
            className={`inline-grid h-[22px] place-items-center ${dense ? "w-[36px]" : "w-[40px]"}`}
          >
            <SpeedBars level={speedLevel(row.wait)} className="h-[12px]!" />
          </span>
          <span
            {...vt(id, "s")}
            className="whitespace-nowrap text-label text-ink-3 tabular-nums"
          >
            {sizeOf(row.model)}
          </span>
        </span>
      </span>
    );
  };

  /** A pick that isn't selected: role, name and few words, then columns. */
  const card = (row: ModelRow, role: PickRole) => {
    const id = row.model.id;
    const dim = row.missing.length > 0;
    return (
      <button
        key={id}
        type="button"
        role="radio"
        aria-checked="false"
        {...vt(id)}
        data-vt-row=""
        onClick={() => choose(id, true)}
        className="grid w-full min-w-0 cursor-pointer grid-cols-[18px_minmax(0,1fr)_auto] items-center gap-x-[10px] rounded-card border-[1.5px] border-line bg-background px-5 py-[8px] text-start text-small leading-[1.5] text-text hover:border-plum-hover hover:bg-wash [view-transition-class:mf-row_mf-box]"
      >
        {mark(id, false)}
        <span className="flex min-w-0 flex-col items-start">
          <span className="flex min-w-0 max-w-full items-baseline gap-[8px] whitespace-nowrap">
            <b {...vt(id, "r")} className={roleClass}>
              {t(`onboarding.model.roles.${ROLE_KEYS[role]}`)}
            </b>
          </span>
          <span className="flex min-w-0 max-w-full items-baseline gap-[8px] whitespace-nowrap">
            <span
              {...vt(id, "n")}
              className={`min-w-0 shrink overflow-hidden text-ellipsis text-small font-semibold text-text ${dim ? "opacity-60" : ""}`}
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
      </button>
    );
  };

  /** One line: role, few words (or the name), one grade, speed and size. */
  const oneLine = (row: ModelRow, role: PickRole | "extra") => {
    const id = row.model.id;
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
        className="flex w-full min-w-0 cursor-pointer items-center gap-[10px] whitespace-nowrap rounded-card border-[1.5px] border-line bg-background px-5 py-[8px] text-start text-small text-text hover:border-plum-hover hover:bg-wash [view-transition-class:mf-row_mf-box]"
      >
        {mark(id, false)}
        <b {...vt(id, "r")} className={roleClass}>
          {t(`onboarding.model.roles.${ROLE_KEYS[role]}`)}
        </b>
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
            <GradeChip
              letter={overall.letter}
              label={overall.label}
              title={langs
                .map((l) => `${nameOf(l)} ${gradeFor(row.model, l).label}`)
                .join(" · ")}
            />
            <SpeedBars level={speedLevel(row.wait)} />
          </span>
          <span {...vt(id, "s")} className={`${sizeClass} ms-0`}>
            {sizeOf(row.model)}
          </span>
        </span>
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
  const primary = (
    <Button variant="primary" size="lg" onClick={() => onChoose(selectedId)}>
      {continuing
        ? t("onboarding.shell.continue")
        : t("onboarding.model.download", { size: sizeOf(selected) })}
    </Button>
  );
  const hint = continuing ? "" : t("onboarding.model.hint");

  const title = (
    <ScreenTitle>
      <span data-vt="mf-title">{t("onboarding.model.title")}</span>
    </ScreenTitle>
  );

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
        pill={pill}
        onBack={onBack}
        hint={hint}
        primary={primary}
      >
        <ScreenMain ref={rootRef} data-model-root="" align="stretch">
          {title}
          <ScreenSub>{t("onboarding.model.subtitle")}</ScreenSub>
          <ScreenGap />
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
          ...row.model.languages.flatMap((l) => [
            languageName(l, ui),
            languageName(l, "en"),
          ]),
        ]
          .join(" ")
          .toLowerCase()
          .includes(w),
      );
  const picked = groups.picks.filter(matches);
  const others = groups.others.filter(matches);
  const partial = groups.partial.filter(matches);
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

  const chipCols = dense
    ? "grid-cols-[repeat(var(--n),26px)_36px_48px] gap-x-[3px]"
    : "grid-cols-[repeat(var(--n),32px)_40px_52px] gap-x-[6px]";
  const chipStyle = { "--n": langs.length } as React.CSSProperties;

  const tableRow = (row: ModelRow) => {
    const id = row.model.id;
    const on = id === selectedId;
    const dim = row.missing.length > 0;
    return (
      <button
        key={id}
        type="button"
        role="radio"
        aria-checked={on}
        {...vt(id)}
        data-vt-row=""
        onClick={() => select(id)}
        className={`relative grid w-full min-w-0 flex-none cursor-pointer grid-cols-[18px_minmax(0,1fr)_auto] items-center gap-x-[10px] rounded-control px-[14px] py-[9px] text-start text-small leading-[1.5] text-text [view-transition-class:mf-row_mf-box] ${
          on ? "is-on bg-pick-row" : "hover:bg-wash"
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
            return (
              <GradeChip
                key={l}
                letter={grade.letter}
                label={grade.label}
                dense={dense}
              />
            );
          })}
          <span
            title={speedText(row.wait)}
            className={`inline-grid h-[22px] place-items-center ${dense ? "w-[36px]" : "w-[40px]"}`}
          >
            <SpeedBars level={speedLevel(row.wait)} className="h-[12px]!" />
          </span>
          <span
            {...vt(id, "s")}
            className="justify-self-end whitespace-nowrap text-label text-ink-3 tabular-nums"
          >
            {sizeOf(row.model)}
          </span>
        </span>
      </button>
    );
  };

  const empty = !picked.length && !others.length && !partial.length;

  return (
    <OnboardingShell
      progress={progress}
      pill={pill}
      onBack={close}
      hint={hint}
      primary={primary}
    >
      <ScreenMain ref={rootRef} data-model-root="" wide align="stretch">
        {title}
        <div
          data-vt="mf-tools"
          className="mt-lead flex items-center gap-[10px] text-start"
        >
          <label className="relative min-w-0 flex-1">
            <span className="pointer-events-none absolute start-[11px] top-1/2 grid -translate-y-1/2 text-ink-3">
              <SearchIcon />
            </span>
            {/* Quiet on focus: the caret says where typing goes, and the
                picked row below stays the one marked thing. */}
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
              className="h-[34px] w-full rounded-lg border border-line-strong bg-background ps-[33px] pe-[12px] text-small text-text outline-none placeholder:text-ink-3"
            />
          </label>
        </div>
        {!empty && (
          // The column heads sit above the list, not in it, so nothing
          // scrolls behind or past them: rows are cut off at the rule. The
          // same 4px bleed and scrollbar gutter as the list keep the
          // columns over theirs.
          <div className="-mx-[4px] mt-[4px] flex-none overflow-y-hidden px-[4px] [scrollbar-gutter:stable] [scrollbar-width:thin]">
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
                <span className={`${CODE_CLASS} justify-self-end`}>
                  {t("onboarding.model.size")}
                </span>
              </span>
            </div>
          </div>
        )}
        <div
          ref={listRef}
          role="radiogroup"
          aria-label={t("onboarding.model.allLabel")}
          className="model-flow-scroll relative -mx-[4px] flex min-h-0 flex-1 flex-col gap-[4px] overflow-y-auto scroll-pt-[12px] px-[4px] pt-[12px] pb-[12px] text-start [scrollbar-width:thin] [scrollbar-gutter:stable]"
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
              {t("onboarding.model.noMatch", { query: query.trim() })}
            </div>
          )}
        </div>
      </ScreenMain>
    </OnboardingShell>
  );
};

const CODE_CLASS =
  "text-tag font-semibold leading-[14px] tracking-[0.02em] text-ink-3";
