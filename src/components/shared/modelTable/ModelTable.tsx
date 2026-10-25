import React, {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import { useTranslation } from "react-i18next";
import { Languages, Radio as LiveIcon, Search } from "lucide-react";
import { Tooltip } from "@/components/ui/Tooltip";
import { languageName } from "@/lib/constants/languages";
import { CODE_CLASS, Codes, rowGrid } from "./columns";
import { ModelTableContext, type TableModel } from "./context";
import type { Grading } from "./grading";
import { ScrollThumb } from "./ScrollThumb";

export interface Filters {
  query: string;
  /** Only models that preview your words while you talk. */
  preview: boolean;
  /** Only models that translate. */
  translate: boolean;
}
export const NO_FILTERS: Filters = {
  query: "",
  preview: false,
  translate: false,
};

/** The search and its filters, and whether a model is kept by them. */
export const useModelFilters = () => {
  const { t, i18n } = useTranslation();
  const [filters, setFilters] = useState<Filters>(NO_FILTERS);
  const q = filters.query.trim().toLowerCase();
  const matches = (e: TableModel) => {
    if (filters.preview && !e.streaming) return false;
    if (filters.translate && !e.translate) return false;
    if (!q) return true;
    const hay = [
      e.name,
      e.description,
      e.streaming ? t("onboarding.model.tags.preview") : "",
      e.translate ? t("onboarding.model.tags.translate") : "",
      ...e.languages.flatMap((l) => [
        languageName(l, i18n.language),
        languageName(l, "en"),
      ]),
    ]
      .join(" ")
      .toLowerCase();
    return q.split(/\s+/).every((w) => hay.includes(w));
  };
  return {
    filters,
    setFilters,
    searching: !!q || filters.preview || filters.translate,
    matches,
  };
};

export interface TableSection {
  key: string;
  /** Its name: in the heads while it's the one under them, else its line. */
  label: React.ReactNode;
  rows: TableModel[];
  /** Small print over its rows. */
  note?: React.ReactNode;
  /** Under its rows (the Models page's empty note, Show more). */
  after?: React.ReactNode;
}

/** The small print, its words over the names (14 + 18 + 10, in the list's 6). */
export const Note: React.FC<{ children: React.ReactNode }> = ({ children }) => (
  <p className="ps-[42px] pe-[12px] pt-[2px] pb-[6px] text-small text-ink-3">
    {children}
  </p>
);

/** A printable key with no shortcut modifier, Space aside. */
const isTyping = (e: KeyboardEvent) =>
  e.key.length === 1 && e.key !== " " && !e.metaKey && !e.ctrlKey && !e.altKey;

interface ModelTableProps {
  grading: Grading;
  filters: Filters;
  setFilters: Dispatch<SetStateAction<Filters>>;
  /** The sections to show, in order; empty ones are the page's to drop. */
  sections: TableSection[];
  renderRow: (entry: TableModel) => React.ReactNode;
  /** The delete/stop column's head; without it there's no such column. */
  actionsHead?: React.ReactNode;
  /** After the last section, under a rule. */
  footer?: React.ReactNode;
  /** Escape with nothing in the search. */
  onEscape?: () => void;
  /** Into the search once it's shown, as Show all opens. */
  focusOnMount?: boolean;
  /**
   * TEMPORARY: the Models page's 1px edge in Settings' grey, to sit with the
   * rest of Settings until its redesign. Remove with it.
   */
  thinEdge?: boolean;
}

/**
 * Every model, the same on onboarding's Show all and the Models page: the
 * search with its filters, then one scrolling list in sections under column
 * heads that stay on top and name the section under them. The list fades
 * at either end while there's more past it. Typing anywhere goes into the
 * search; ↑↓ there move a highlight, Enter presses it; Escape clears the
 * search, then hands over to the page.
 */
export const ModelTable: React.FC<ModelTableProps> = ({
  grading,
  filters,
  setFilters,
  sections,
  renderRow,
  actionsHead,
  footer,
  onEscape,
  focusOnMount = false,
  thinEdge = false,
}) => {
  const { t } = useTranslation();
  const [highlighted, setHighlighted] = useState<string | null>(null);
  const [current, setCurrent] = useState(sections[0]?.key ?? "");
  const searchRef = useRef<HTMLInputElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const headsRef = useRef<HTMLDivElement>(null);
  const empty = !sections.some((s) => s.rows.length);
  const actions = !!actionsHead;

  // Typing anywhere goes into the search; Escape clears it, then is the
  // page's. Space stays with what has focus: it presses a filter or a row.
  const escape = useRef(onEscape);
  escape.current = onEscape;
  const query = useRef(filters.query);
  query.current = filters.query;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (query.current) {
          e.preventDefault();
          setFilters((f) => ({ ...f, query: "" }));
        } else if (escape.current) {
          e.preventDefault();
          escape.current();
        }
        return;
      }
      const el = e.target as HTMLElement | null;
      if (el && /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName)) return;
      if (isTyping(e)) searchRef.current?.focus({ preventScroll: true });
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [setFilters]);

  // Shown: into the search, and the picked row in view. In a layout effect,
  // so a move into the list (onboarding's) starts from where it'll be.
  useLayoutEffect(() => {
    if (focusOnMount) {
      const input = searchRef.current;
      input?.focus({ preventScroll: true });
      input?.setSelectionRange(input.value.length, input.value.length);
    }
    // Not laid out yet (it can take a frame or two): again next frame.
    let frame = 0;
    let tries = 0;
    const reveal = () => {
      const list = listRef.current;
      if (!list) return;
      if (!list.clientHeight && tries++ < 10) {
        frame = requestAnimationFrame(reveal);
        return;
      }
      const row = list.querySelector<HTMLElement>(
        '[data-row][aria-checked="true"]',
      );
      if (!row) return;
      const bottom = row.offsetTop + row.offsetHeight + 12 - list.clientHeight;
      if (list.scrollTop < bottom) list.scrollTop = bottom;
    };
    reveal();
    return () => cancelAnimationFrame(frame);
  }, []); // Once, when shown.

  // The ends fade while there's more past them, and the heads name the
  // section under them, as a sticky heading does: the first at the top,
  // then each next one once its own line is half under them. Checked on
  // every render (a search or filter changes what's there), on scroll and
  // on resize.
  const follow = useRef(() => {});
  follow.current = () => {
    const panel = panelRef.current;
    const list = listRef.current;
    if (!panel || !list) return;
    panel.toggleAttribute("data-scrolled", list.scrollTop > 0);
    list.toggleAttribute(
      "data-more",
      list.scrollTop + list.clientHeight < list.scrollHeight - 1,
    );
    const edge = headsRef.current?.getBoundingClientRect().bottom;
    let key = sections[0]?.key ?? "";
    if (edge != null)
      for (const el of list.querySelectorAll<HTMLElement>("[data-section]"))
        if (el.getBoundingClientRect().top + el.offsetHeight / 2 < edge)
          key = el.dataset.section!;
    setCurrent(key);
  };
  useLayoutEffect(() => follow.current());
  useEffect(() => {
    const list = listRef.current;
    if (!list) return;
    const onScroll = () => follow.current();
    list.addEventListener("scroll", onScroll, { passive: true });
    const resized = new ResizeObserver(onScroll);
    resized.observe(list);
    return () => {
      list.removeEventListener("scroll", onScroll);
      resized.disconnect();
    };
  }, []);
  const heading = sections.find((s) => s.key === current) ?? sections[0];

  /** From the search: ↑↓ move the highlight, from the picked row at first. */
  const step = (d: number) => {
    const rows = [
      ...(listRef.current?.querySelectorAll<HTMLElement>("[data-row]") ?? []),
    ];
    if (!rows.length) return;
    let i = rows.findIndex((el) => el.dataset.row === highlighted);
    if (i < 0)
      i = rows.findIndex((el) => el.getAttribute("aria-checked") === "true");
    const next =
      i < 0
        ? rows[d > 0 ? 0 : rows.length - 1]
        : rows[Math.max(0, Math.min(rows.length - 1, i + d))];
    setHighlighted(next.dataset.row ?? null);
    next.scrollIntoView({ block: "nearest" });
  };
  const press = () => {
    if (!highlighted) return;
    listRef.current
      ?.querySelector<HTMLElement>(`[data-row="${CSS.escape(highlighted)}"]`)
      ?.click();
  };

  /** A filter after the search's divider: icon and word; pressed fills grey. */
  const filter = (key: "preview" | "translate", Icon: typeof LiveIcon) => (
    <Tooltip text={t(`onboarding.model.tags.${key}Title`)}>
      <button
        type="button"
        aria-pressed={filters[key]}
        onClick={() => {
          setFilters((f) => ({ ...f, [key]: !f[key] }));
          setHighlighted(null);
          if (listRef.current) listRef.current.scrollTop = 0;
        }}
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
    <ModelTableContext.Provider value={{ grading, actions, highlighted }}>
      <div
        ref={panelRef}
        className={`group/list flex min-h-0 w-full flex-1 flex-col overflow-hidden rounded-panel bg-background text-start ${
          thinEdge ? "border border-line" : "border-[1.5px] border-line-strong"
        }`}
      >
        {/* The search is the first row, the heads and list under a rule. The
            magnifier sits in the marks' column and the typing starts where
            the names do. */}
        <div className="flex-none border-b border-line px-[6px]">
          <div className="flex h-[46px] items-center gap-[4px] ps-[14px] pe-[4px]">
            <label className="flex h-full min-w-0 flex-1 cursor-text items-center gap-[10px]">
              <span className="grid w-[18px] shrink-0 place-items-center text-ink-3">
                <Search size={15} aria-hidden="true" />
              </span>
              {/* Quiet on focus: the caret says where typing goes. No ×:
                  Escape clears. */}
              <input
                ref={searchRef}
                type="search"
                value={filters.query}
                placeholder={t("onboarding.model.search")}
                aria-label={t("onboarding.model.search")}
                onChange={(e) => {
                  const value = e.target.value;
                  setFilters((f) => ({ ...f, query: value }));
                  setHighlighted(null);
                  if (listRef.current) listRef.current.scrollTop = 0;
                }}
                onKeyDown={(e) => {
                  const d =
                    e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
                  if (d) {
                    e.preventDefault();
                    step(d);
                  } else if (e.key === "Enter" && highlighted) {
                    e.preventDefault();
                    press();
                  }
                }}
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
        </div>
        {!empty && (
          // The heads, over the list rather than in it, so rows only ever
          // pass out of sight at the list's edge (scrolling or bouncing).
          // Under them, once rows pass beneath, a soft edge instead of a
          // hard line; it starts 1px up, inside them, so no sliver of a row
          // shows between the two at fractional pixels.
          <div className="relative z-[1] flex-none bg-background px-[6px] after:pointer-events-none after:absolute after:inset-x-0 after:top-[calc(100%-1px)] after:h-[21px] after:bg-linear-to-b after:from-background after:to-transparent after:opacity-0 after:transition-opacity group-data-scrolled/list:after:opacity-100">
            <div
              ref={headsRef}
              className={`${rowGrid(actions)} h-[32px] px-[14px]`}
            >
              <span />
              {/* Keyed by section: a new name mounts and fades in. */}
              <span
                key={heading?.key}
                className={`flex min-w-0 items-center truncate motion-safe:animate-fade ${CODE_CLASS}`}
              >
                {heading?.label}
              </span>
              <Codes langs={grading.langs} />
              {actions && (
                <span className="grid place-items-center">{actionsHead}</span>
              )}
            </div>
          </div>
        )}
        {/* The list's scrollbar floats over it (ScrollThumb), so rows and
            dividers run the full width on every platform. */}
        <div className="group/scroll relative flex min-h-0 flex-1 flex-col">
          <div
            ref={listRef}
            data-model-scroll=""
            role="radiogroup"
            aria-label={t("onboarding.model.allLabel")}
            // The pointer takes over from the keys.
            onPointerMove={() => highlighted && setHighlighted(null)}
            className="relative flex min-h-0 flex-1 flex-col overflow-y-auto scroll-pt-[20px] scroll-pb-[12px] px-[6px] pb-[6px] [scrollbar-width:none] [&::-webkit-scrollbar]:hidden data-more:[mask-image:linear-gradient(to_bottom,black_calc(100%-40px),transparent_calc(100%-6px))]"
          >
            {empty && (
              <div className="px-[12px] py-[28px] text-center text-small text-ink-2">
                {filters.query.trim()
                  ? t("onboarding.model.noMatch", {
                      query: filters.query.trim(),
                    })
                  : t("onboarding.model.noFilterMatch")}
              </div>
            )}
            {!empty &&
              sections.map((s, i) => (
                <section key={s.key} className="flex-none">
                  {/* The first section's name is the heads'. The others open
                    with a rule across the list and their name over the
                    names; scrolled under the heads, the heads take it. */}
                  {i > 0 && (
                    <div
                      data-section={s.key}
                      className={`${rowGrid(actions)} -mx-[6px] mt-[6px] h-[33px] border-t border-line px-[20px]`}
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
                    {s.rows.map(renderRow)}
                  </div>
                  {s.after}
                </section>
              ))}
            {!empty && footer && (
              <div className="-mx-[6px] mt-[6px] flex-none border-t border-line px-[6px] pt-[8px]">
                {footer}
              </div>
            )}
          </div>
          <ScrollThumb listRef={listRef} />
        </div>
      </div>
    </ModelTableContext.Provider>
  );
};
