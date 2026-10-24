import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Languages, Radio as LiveIcon, RefreshCw, Search } from "lucide-react";
import { Tooltip } from "@/components/ui/Tooltip";
import { languageName } from "@/lib/constants/languages";
import { useModelStore } from "@/stores/modelStore";
import RetiredModelsNotice from "./RetiredModelsNotice";
import { ModelRow } from "./ModelRow";
import { Legend } from "./Legend";
import { CODE_CLASS, Codes, ROW_GRID } from "./columns";
import { useModelEntries, type Entry } from "./modelEntries";

/** Past this many models on disk, the rest fold under "Show N more". */
const YOURS_LIMIT = 5;

interface Filters {
  query: string;
  preview: boolean;
  translate: boolean;
}
const NO_FILTERS: Filters = { query: "", preview: false, translate: false };

/** The small print, its words over the names (14 + 18 + 10, in the panel's 6). */
const Note: React.FC<{ children: React.ReactNode }> = ({ children }) => (
  <p className="ps-[42px] pe-[12px] pt-[2px] pb-[6px] text-small text-ink-3">
    {children}
  </p>
);

/** Look for model files added outside Handy: quiet, beside its section's name. */
const Rescan: React.FC = () => {
  const { t } = useTranslation();
  const rescanning = useModelStore((s) => s.isRescanning);
  return (
    <Tooltip text={t("settings.models.rescan.tooltip")}>
      <button
        type="button"
        onClick={() => useModelStore.getState().rescanLocalModels()}
        disabled={rescanning}
        aria-label={t("settings.models.rescan.tooltip")}
        className="ms-[6px] grid size-[22px] cursor-pointer place-items-center rounded-full text-ink-3 hover:bg-wash hover:text-text disabled:cursor-default focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
      >
        <RefreshCw
          className={`size-[12px] ${rescanning ? "animate-spin" : ""}`}
          aria-hidden="true"
        />
      </button>
    </Tooltip>
  );
};

interface Section {
  key: string;
  label: React.ReactNode;
  rows: Entry[];
  note?: string;
}

interface ModelsSettingsProps {
  /** Where the languages are changed (General). */
  onChangeLanguages?: () => void;
}

/**
 * Every model, as onboarding's Show all panel: the search its first row, then
 * the column heads, both pinned while the page scrolls; one list in sections
 * (on this computer, for your languages, missing one of them, and untested
 * models only when searching), the heads naming the section under them.
 */
export const ModelsSettings: React.FC<ModelsSettingsProps> = ({
  onChangeLanguages,
}) => {
  const { t, i18n } = useTranslation();
  const loading = useModelStore((s) => s.loading);
  const { facts, lists } = useModelEntries();
  const [filters, setFilters] = useState<Filters>(NO_FILTERS);
  const [more, setMore] = useState(false);
  const [stuck, setStuck] = useState(false);
  /** The section under the pinned heads, whose name the heads carry. */
  const [current, setCurrent] = useState("yours");
  const searchRef = useRef<HTMLInputElement>(null);
  const topRef = useRef<HTMLDivElement>(null);
  const headsRef = useRef<HTMLDivElement>(null);
  const bodyRef = useRef<HTMLDivElement>(null);
  const { langs } = facts;
  const ui = i18n.language;
  const listOf = (codes: string[]) =>
    new Intl.ListFormat(ui, { type: "conjunction" }).format(
      codes.map((c) => languageName(c, ui)),
    );

  // Typing anywhere on the page goes into the search; Escape clears it.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement | null;
      if (e.key === "Escape" && filters.query) {
        setFilters((f) => ({ ...f, query: "" }));
        return;
      }
      if (el && /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName)) return;
      if (e.key.length !== 1 || e.key === " ") return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      searchRef.current?.focus();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  const searching =
    !!filters.query.trim() || filters.preview || filters.translate;
  const matches = (e: Entry) => {
    if (filters.preview && !e.streaming) return false;
    if (filters.translate && !e.translate) return false;
    const q = filters.query.trim().toLowerCase();
    if (!q) return true;
    const hay = [
      e.name,
      e.description,
      e.streaming ? t("onboarding.model.tags.preview") : "",
      e.translate ? t("onboarding.model.tags.translate") : "",
      ...e.languages.flatMap((l) => [
        languageName(l, ui),
        languageName(l, "en"),
      ]),
    ]
      .join(" ")
      .toLowerCase();
    return q.split(/\s+/).every((w) => hay.includes(w));
  };
  const keep = (xs: Entry[]) => xs.filter(matches);

  const sections: Section[] = searching
    ? [
        {
          key: "yours",
          label: t("settings.models.sections.yours"),
          rows: keep(lists.yours),
        },
        {
          key: "fits",
          label: t("settings.models.sections.fits"),
          rows: keep(lists.fits),
        },
        {
          key: "partial",
          label: t("settings.models.sections.missing"),
          rows: keep(lists.partial),
        },
        {
          key: "untested",
          label: t("settings.models.sections.untested"),
          rows: filters.query.trim() ? keep(lists.untested) : [],
          note: t("settings.models.untestedNote"),
        },
      ]
    : [
        {
          key: "yours",
          label: (
            <>
              {t("settings.models.sections.yours")}
              <Rescan />
            </>
          ),
          rows: more ? lists.yours : lists.yours.slice(0, YOURS_LIMIT),
        },
        {
          key: "fits",
          label: t("settings.models.sections.fits"),
          rows: lists.fits,
        },
        {
          key: "partial",
          label: t("settings.models.sections.missing"),
          rows: lists.partial,
        },
      ];

  // Yours always shows at rest (its empty note says how to get one); a
  // search shows only the sections it found something in.
  const visible = sections.filter(
    (s) => s.rows.length || (s.key === "yours" && !searching),
  );

  // Pinned with rows passing under it: a rule under the heads, as Show all's
  // list is cut off at its top edge. At rest there's nothing under it to cut.
  // The heads name the section you're in, as a sticky heading does: the
  // first at rest, then each next one once its own line is half under them.
  const follow = useRef(() => {});
  follow.current = () => {
    const top = topRef.current;
    const heads = headsRef.current;
    const body = bodyRef.current;
    const scroller = top?.closest<HTMLElement>(".overflow-y-auto");
    if (!top || !heads || !body || !scroller) return;
    setStuck(
      top.getBoundingClientRect().top <=
        scroller.getBoundingClientRect().top + 0.5,
    );
    const edge = heads.getBoundingClientRect().bottom;
    let key = visible[0]?.key ?? "";
    for (const el of body.querySelectorAll<HTMLElement>("[data-section]"))
      if (el.getBoundingClientRect().top + el.offsetHeight / 2 < edge)
        key = el.dataset.section!;
    setCurrent(key);
  };
  useLayoutEffect(() => follow.current());
  useEffect(() => {
    const scroller = topRef.current?.closest<HTMLElement>(".overflow-y-auto");
    if (!scroller) return;
    const onScroll = () => follow.current();
    scroller.addEventListener("scroll", onScroll, { passive: true });
    return () => scroller.removeEventListener("scroll", onScroll);
  }, [loading]);
  const heading = visible.find((s) => s.key === current) ?? visible[0];

  if (loading) {
    return (
      <div className="max-w-3xl w-full mx-auto">
        <div className="flex items-center justify-center py-16">
          <div className="w-8 h-8 border-2 border-logo-primary border-t-transparent rounded-full animate-spin" />
        </div>
      </div>
    );
  }

  /** Show all's filter: icon and word after the divider; pressed fills grey. */
  const filter = (key: "preview" | "translate", Icon: typeof LiveIcon) => (
    <Tooltip text={t(`onboarding.model.tags.${key}Title`)}>
      <button
        type="button"
        aria-pressed={filters[key]}
        onClick={() => setFilters((f) => ({ ...f, [key]: !f[key] }))}
        className={`flex h-[26px] shrink-0 cursor-pointer items-center gap-[6px] whitespace-nowrap rounded-[7px] px-[8px] text-small font-medium transition-colors focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary ${
          filters[key]
            ? "bg-line text-text"
            : "text-ink-3 hover:bg-wash hover:text-text"
        }`}
      >
        <Icon aria-hidden="true" className="size-[14px]" />
        {t(`onboarding.model.tags.${key}`)}
      </button>
    </Tooltip>
  );

  return (
    <div className="max-w-3xl w-full mx-auto">
      {/* Whose page it is: graded for your languages, timed for a computer
          like this one, and where to change them. */}
      <div className="mb-[14px] px-[12px]">
        <h1 className="mb-[2px] text-xl font-semibold">
          {t("settings.models.title")}
        </h1>
        <p className="text-sm text-ink-2">
          {langs.length > 3 ? (
            <Tooltip text={listOf(langs)}>
              <span>
                {t("settings.models.lensMany", { count: langs.length })}
              </span>
            </Tooltip>
          ) : (
            t("settings.models.lens", { languages: listOf(langs) })
          )}
          {onChangeLanguages && (
            <>
              {" "}
              <button
                type="button"
                onClick={onChangeLanguages}
                className="cursor-pointer text-text underline decoration-line-strong underline-offset-[3px] hover:decoration-text focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
              >
                {t("settings.models.changeLanguages")}
              </button>
            </>
          )}
        </p>
      </div>
      <div className="mx-[12px] mb-[12px] empty:hidden">
        <RetiredModelsNotice />
      </div>

      {/* The panel's top, pinned: the search, then the heads, named for
          the section under them. */}
      <div
        ref={topRef}
        className="sticky -top-[16px] z-20 bg-background pt-[16px]"
      >
        <div className="rounded-t-panel border-[1.5px] border-b-0 border-line-strong bg-background">
          {/* The magnifier sits in the marks' column and the typing starts
              where the names do. */}
          <div className="flex h-[46px] items-center gap-[4px] border-b border-line ps-[20px] pe-[10px]">
            <label className="flex h-full min-w-0 flex-1 cursor-text items-center gap-[10px]">
              <span className="grid w-[18px] shrink-0 place-items-center text-ink-3">
                <Search size={15} aria-hidden="true" />
              </span>
              <input
                ref={searchRef}
                type="search"
                value={filters.query}
                placeholder={t("onboarding.model.searchPlaceholder")}
                aria-label={t("onboarding.model.searchLabel")}
                onChange={(e) =>
                  setFilters((f) => ({ ...f, query: e.target.value }))
                }
                className="h-full min-w-0 flex-1 bg-transparent text-body text-text outline-none placeholder:text-ink-3 [&::-webkit-search-cancel-button]:appearance-none"
              />
            </label>
            <span
              aria-hidden="true"
              className="me-[2px] h-[16px] w-px shrink-0 bg-line"
            />
            {filter("preview", LiveIcon)}
            {filter("translate", Languages)}
          </div>
          <div
            className={`border-b px-[6px] transition-[border-color] ${stuck ? "border-line" : "border-transparent"}`}
          >
            <div ref={headsRef} className={`${ROW_GRID} h-[32px] px-[14px]`}>
              <span />
              {/* Keyed by section: a new name mounts and fades in. */}
              <span
                key={heading?.key}
                className={`flex min-w-0 items-center truncate motion-safe:animate-fade ${CODE_CLASS}`}
              >
                {heading?.label}
              </span>
              <Codes langs={langs} />
              <span className="grid place-items-center">
                <Legend tier={facts.tier} />
              </span>
            </div>
          </div>
        </div>
      </div>

      {/* flow-root: the first row's margin stays inside the edge. */}
      <div
        ref={bodyRef}
        role="radiogroup"
        aria-label={t("onboarding.model.allLabel")}
        className="flow-root rounded-b-panel border-[1.5px] border-t-0 border-line-strong px-[6px] pb-[6px]"
      >
        {!visible.some((s) => s.rows.length) && (
          <div className="px-[12px] py-[28px] text-center text-small text-ink-2">
            {filters.query.trim()
              ? t("onboarding.model.noMatch", { query: filters.query.trim() })
              : t("onboarding.model.noFilterMatch")}
          </div>
        )}
        {visible.map((s, i) => (
          <section key={s.key}>
            {/* The first section's name is the heads'. The others open with
                Show all's quiet line, a rule across and its words over the
                names; scrolled under the heads, the heads take its name. */}
            {i > 0 && (
              <div
                data-section={s.key}
                className={`${ROW_GRID} mt-[6px] h-[33px] border-t border-line px-[14px]`}
              >
                <span />
                <span
                  className={`flex min-w-0 items-center truncate ${CODE_CLASS}`}
                >
                  {s.label}
                </span>
              </div>
            )}
            {s.note && <Note>{s.note}</Note>}
            <div className="flex flex-col gap-[4px]">
              {s.rows.map((e) => (
                <ModelRow key={e.id} entry={e} facts={facts} />
              ))}
            </div>
            {s.key === "yours" && !s.rows.length && !searching && (
              <Note>{t("settings.models.emptyYours")}</Note>
            )}
            {s.key === "yours" &&
              !searching &&
              lists.yours.length > YOURS_LIMIT && (
                <button
                  type="button"
                  onClick={() => setMore((m) => !m)}
                  className="ms-[40px] mt-[2px] flex h-[28px] cursor-pointer items-center gap-[8px] rounded-[7px] px-[2px] text-small font-medium text-ink-2 hover:text-text focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
                >
                  {more
                    ? t("settings.models.showFewer")
                    : t("settings.models.showMore", {
                        count: lists.yours.length - YOURS_LIMIT,
                      })}
                  <span
                    aria-hidden="true"
                    className={`inline-block size-[6px] shrink-0 border-e-[1.5px] border-b-[1.5px] border-current ${more ? "mt-[3px] -rotate-[135deg]" : "-mt-[3px] rotate-45"}`}
                  />
                </button>
              )}
          </section>
        ))}
        {!searching && lists.untested.length > 0 && (
          <div className="mt-[6px] border-t border-line pt-[8px]">
            <Note>
              {t("settings.models.untestedHidden", {
                count: lists.untested.length,
              })}
            </Note>
          </div>
        )}
      </div>
    </div>
  );
};
