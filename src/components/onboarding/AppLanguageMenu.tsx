import React, { useEffect, useId, useRef, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import { Check, ChevronDown, Globe } from "lucide-react";
import { SUPPORTED_LANGUAGES } from "@/i18n";
import { cjkSubsetFont } from "@/lib/utils/cjkFont";

interface AppLanguageMenuProps {
  /** The language the menus are in. */
  value: string;
  onChange: (code: string) => void;
}

/** App language menu; custom because Windows paints a <select> popup light. */
export const AppLanguageMenu: React.FC<AppLanguageMenuProps> = ({
  value,
  onChange,
}) => {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const id = useId();

  const current =
    SUPPORTED_LANGUAGES.find((l) => l.code === value) ?? SUPPORTED_LANGUAGES[0];
  const selectedIndex = Math.max(
    0,
    SUPPORTED_LANGUAGES.findIndex((l) => l.code === value),
  );

  const show = () => {
    setActive(selectedIndex);
    setOpen(true);
  };
  const close = (refocus = true) => {
    setOpen(false);
    if (refocus) triggerRef.current?.focus({ preventScroll: true });
  };
  const pick = (code: string) => {
    if (code !== value) onChange(code);
    close();
  };

  // Focus the list, current language in view.
  useEffect(() => {
    if (!open) return;
    listRef.current?.focus({ preventScroll: true });
    listRef.current
      ?.querySelector('[aria-selected="true"]')
      ?.scrollIntoView({ block: "center" });
  }, [open]);

  // Keep the highlight in view.
  useEffect(() => {
    if (!open) return;
    document
      .getElementById(`${id}-${active}`)
      ?.scrollIntoView({ block: "nearest" });
  }, [open, active, id]);

  // Close on outside press.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", onDown);
    return () => document.removeEventListener("pointerdown", onDown);
  }, [open]);

  const onListKeyDown = (e: React.KeyboardEvent) => {
    const last = SUPPORTED_LANGUAGES.length - 1;
    const keys: Record<string, () => void> = {
      ArrowDown: () => setActive((i) => Math.min(last, i + 1)),
      ArrowUp: () => setActive((i) => Math.max(0, i - 1)),
      Home: () => setActive(0),
      End: () => setActive(last),
      Enter: () => pick(SUPPORTED_LANGUAGES[active].code),
      " ": () => pick(SUPPORTED_LANGUAGES[active].code),
      Escape: () => close(),
      Tab: () => close(),
    };
    const handle = keys[e.key];
    if (handle) {
      if (e.key !== "Tab") e.preventDefault();
      handle();
      return;
    }
    // Typeahead.
    if (e.key.length === 1 && !e.metaKey && !e.ctrlKey && !e.altKey) {
      const ch = e.key.toLocaleLowerCase();
      const n = SUPPORTED_LANGUAGES.length;
      for (let step = 1; step <= n; step++) {
        const i = (active + step) % n;
        if (
          SUPPORTED_LANGUAGES[i].nativeName.toLocaleLowerCase().startsWith(ch)
        ) {
          setActive(i);
          break;
        }
      }
    }
  };

  return (
    <div ref={rootRef} className="relative -me-[8px] shrink-0">
      <button
        ref={triggerRef}
        type="button"
        title={t("onboarding.welcome.appLanguage")}
        aria-label={t("onboarding.welcome.appLanguage")}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? `${id}-list` : undefined}
        onClick={() => (open ? close() : show())}
        onKeyDown={(e) => {
          if (!open && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
            e.preventDefault();
            show();
          }
        }}
        className={`flex cursor-pointer items-center gap-[6px] rounded-lg px-[8px] py-[4px] text-small transition-colors hover:bg-wash hover:text-text focus:outline-none focus-visible:bg-wash focus-visible:text-text ${
          open ? "bg-wash text-text" : "text-ink-2"
        }`}
      >
        <Globe size={16} aria-hidden="true" />
        <span>
          <Trans
            i18nKey="onboarding.welcome.menusIn"
            values={{ language: current.nativeName }}
            components={{
              b: (
                <b
                  lang={current.code}
                  className={`font-semibold text-text ${cjkSubsetFont(current.nativeName) ?? ""}`}
                />
              ),
            }}
          />
        </span>
        <ChevronDown
          size={14}
          aria-hidden="true"
          className={`transition-transform duration-200 ease-glide ${open ? "rotate-180" : ""}`}
        />
      </button>

      {open && (
        <div
          ref={listRef}
          id={`${id}-list`}
          role="listbox"
          tabIndex={-1}
          aria-label={t("onboarding.welcome.appLanguage")}
          aria-activedescendant={`${id}-${active}`}
          onKeyDown={onListKeyDown}
          className="absolute end-0 top-[calc(100%+6px)] z-50 max-h-[372px] w-[208px] origin-top-right animate-drop overflow-y-auto overscroll-contain rounded-card border border-line-strong bg-background p-[5px] shadow-[0_14px_36px_-10px_rgb(0_0_0/0.35),0_2px_6px_rgb(0_0_0/0.08)] outline-none rtl:origin-top-left"
        >
          {SUPPORTED_LANGUAGES.map((l, i) => {
            const selected = l.code === value;
            return (
              <div
                key={l.code}
                id={`${id}-${i}`}
                role="option"
                aria-selected={selected}
                onPointerMove={() => setActive(i)}
                onClick={() => pick(l.code)}
                className={`grid h-[34px] cursor-pointer grid-cols-[18px_minmax(0,1fr)] items-center gap-[8px] rounded-[8px] ps-[8px] pe-[10px] text-small ${
                  i === active ? "bg-wash" : ""
                }`}
              >
                <span className="grid place-items-center text-plum">
                  {selected && (
                    <Check size={15} strokeWidth={2.5} aria-hidden="true" />
                  )}
                </span>
                {/* Span keeps RTL names aligned with the rest. */}
                <span
                  className={`truncate text-text ${selected ? "font-semibold" : "font-medium"}`}
                >
                  <bdi lang={l.code} className={cjkSubsetFont(l.nativeName)}>
                    {l.nativeName}
                  </bdi>
                </span>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
};
