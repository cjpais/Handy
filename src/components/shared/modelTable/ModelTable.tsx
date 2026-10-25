import React, {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import { useTranslation } from "react-i18next";
import { Search } from "lucide-react";
import { Tooltip } from "@/components/ui/Tooltip";
import { languageName } from "@/lib/constants/languages";
import { CAPABILITIES, CAPABILITY_KEYS, type Capability } from "./capabilities";
import { CODE_CLASS, Codes, rowGrid } from "./columns";
import { ModelTableContext, type TableModel } from "./context";
import type { Grading } from "@/lib/models/grading";
import { ScrollThumb } from "./ScrollThumb";

/** The search, and per capability whether only models with it are kept. */
export type Filters = { query: string } & Record<Capability, boolean>;
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
    const has = (c: Capability) => e[CAPABILITIES[c].flag];
    if (CAPABILITY_KEYS.some((c) => filters[c] && !has(c))) return false;
    if (!q) return true;
    const hay = [
      e.name,
      e.description,
      ...CAPABILITY_KEYS.filter(has).map((c) => t(`modelTable.tags.${c}`)),
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
    searching: !!q || CAPABILITY_KEYS.some((c) => filters[c]),
    matches,
  };
};

export interface TableSection {
  key: string;
  /** Shown in the sticky heads while scrolled under them, else as a divider label. */
  label: React.ReactNode;
  rows: TableModel[];
  /** Small print over its rows. */
  note?: React.ReactNode;
  /** Under its rows (the Models page's empty note, Show more). */
  after?: React.ReactNode;
}

/** A section's small print, indented to align with the names column. */
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
  /** Sections in order; the caller drops empty ones. */
  sections: TableSection[];
  renderRow: (entry: TableModel) => React.ReactNode;
  /** Head for the delete/stop column; omit for no such column. */
  actionsHead?: React.ReactNode;
  /** After the last section, under a rule. */
  footer?: React.ReactNode;
  /** Escape with nothing in the search. */
  onEscape?: () => void;
  /** Focus the search on mount. */
  focusOnMount?: boolean;
  /** TEMPORARY: Settings' thinner edge until the Settings redesign; remove with it. */
  thinEdge?: boolean;
}

/** The searchable, sectioned model list shared by onboarding's Show all and the Models page. */
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

  // Typing anywhere goes into the search; Escape clears it, then falls to the
  // page. Space is left alone so it still presses the focused filter or row.
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

  // Focus the search and scroll the picked row into view. A layout effect, so
  // onboarding's view transition starts from the scrolled position.
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

  // Edge fades and the sticky section name. Re-checked on every render (search
  // and filters change the content), scroll and resize.
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

  /** ↑↓ from the search move the highlight, starting at the picked row. */
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

  /** A toggle filter beside the search. */
  const filter = (key: Capability) => {
    const { Icon } = CAPABILITIES[key];
    return (
      <Tooltip key={key} text={t(`modelTable.tags.${key}Title`)}>
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
          {t(`modelTable.tags.${key}`)}
        </button>
      </Tooltip>
    );
  };

  return (
    <ModelTableContext.Provider value={{ grading, actions, highlighted }}>
      <div
        ref={panelRef}
        className={`group/list flex min-h-0 w-full flex-1 flex-col overflow-hidden rounded-panel bg-background text-start ${
          thinEdge ? "border border-line" : "border-[1.5px] border-line-strong"
        }`}
      >
        {/* The magnifier and input align with the marks and names columns. */}
        <div className="flex-none border-b border-line px-[6px]">
          <div className="flex h-[46px] items-center gap-[4px] ps-[14px] pe-[4px]">
            <label className="flex h-full min-w-0 flex-1 cursor-text items-center gap-[10px]">
              <span className="grid w-[18px] shrink-0 place-items-center text-ink-3">
                <Search size={15} aria-hidden="true" />
              </span>
              {/* No clear button: Escape clears. */}
              <input
                ref={searchRef}
                type="search"
                value={filters.query}
                placeholder={t("modelTable.search")}
                aria-label={t("modelTable.search")}
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
            {CAPABILITY_KEYS.map(filter)}
          </div>
        </div>
        {!empty && (
          // Heads sit outside the list so rows only clip at the list's edge. The
          // fade starts 1px up so no row sliver shows at fractional pixels.
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
        {/* ScrollThumb overlays the list so rows run full width on every platform. */}
        <div className="group/scroll relative flex min-h-0 flex-1 flex-col">
          <div
            ref={listRef}
            data-model-scroll=""
            role="radiogroup"
            aria-label={t("modelTable.allLabel")}
            // The pointer takes over from the keys.
            onPointerMove={() => highlighted && setHighlighted(null)}
            className="relative flex min-h-0 flex-1 flex-col overflow-y-auto scroll-pt-[20px] scroll-pb-[12px] px-[6px] pb-[6px] [scrollbar-width:none] [&::-webkit-scrollbar]:hidden data-more:[mask-image:linear-gradient(to_bottom,black_calc(100%-40px),transparent_calc(100%-6px))]"
          >
            {empty && (
              <div className="px-[12px] py-[28px] text-center text-small text-ink-2">
                {filters.query.trim()
                  ? t("modelTable.noMatch", {
                      query: filters.query.trim(),
                    })
                  : t("modelTable.noFilterMatch")}
              </div>
            )}
            {!empty &&
              sections.map((s, i) => (
                <section key={s.key} className="flex-none">
                  {/* The first section's name lives in the heads; later ones get a divider. */}
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
