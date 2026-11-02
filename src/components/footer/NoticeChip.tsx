import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Notice } from "@/bindings";
import { NoticeBadgeIcon } from "@/components/icons/NoticeBadgeIcon";
import type { SidebarSection } from "@/components/Sidebar";
import { noticeDisplay } from "@/lib/notices";
import { useNoticeStore } from "@/stores/noticeStore";

const levelColor = (notices: Notice[]) =>
  notices.some((notice) => notice.level === "error")
    ? "text-error"
    : "text-warning";

interface NoticeChipProps {
  onShowSection: (section: SidebarSection) => void;
}

const NoticeChip: React.FC<NoticeChipProps> = ({ onShowSection }) => {
  const { t } = useTranslation();
  const notices = useNoticeStore((s) => s.notices);
  const [isOpen, setIsOpen] = useState(false);
  const chipRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (notices.length === 0) setIsOpen(false);
  }, [notices.length]);

  useEffect(() => {
    if (!isOpen) return;
    const handleClickOutside = (event: MouseEvent) => {
      if (!chipRef.current?.contains(event.target as Node)) setIsOpen(false);
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setIsOpen(false);
    };
    document.addEventListener("mousedown", handleClickOutside);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [isOpen]);

  if (notices.length === 0) return null;

  // Plain keys, not plural ones: languages with more plural forms than this
  // app's `_one`/`_other` would fall back to English for a count of 2.
  const errors = notices.filter((notice) => notice.level === "error").length;
  const warnings = notices.length - errors;
  const errorText = t(errors === 1 ? "notices.errorOne" : "notices.errorMany", {
    n: errors,
  });
  const warningText = t(
    warnings === 1 ? "notices.warningOne" : "notices.warningMany",
    { n: warnings },
  );
  const chipText =
    errors > 0 && warnings > 0
      ? t("notices.errorsAndWarnings", {
          errors: errorText,
          warnings: warningText,
        })
      : errors > 0
        ? errorText
        : warningText;

  return (
    <div className="relative shrink-0" ref={chipRef}>
      <button
        type="button"
        aria-expanded={isOpen}
        onClick={() => setIsOpen((open) => !open)}
        className="flex cursor-pointer items-center gap-1.5 whitespace-nowrap text-text transition-colors hover:text-ink-2 focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
      >
        <NoticeBadgeIcon className={`h-3 w-3 ${levelColor(notices)}`} />
        {chipText}
      </button>

      {isOpen && (
        <div className="absolute bottom-full start-0 z-50 mb-2 flex max-h-[60vh] w-80 flex-col gap-2 overflow-y-auto rounded-lg border border-line bg-background p-2 shadow-lg">
          {notices.map((notice) => {
            const { title, explanation, page } = noticeDisplay(notice.code);
            return (
              <div
                key={notice.source}
                className="flex items-start gap-3 rounded-md p-2"
              >
                <NoticeBadgeIcon
                  className={`mt-0.5 h-3 w-3 shrink-0 ${levelColor([notice])}`}
                />
                <div className="min-w-0 flex-1 select-text break-words">
                  <p className="text-sm font-medium text-text">{t(title)}</p>
                  {explanation && <p className="mt-0.5">{explanation(t)}</p>}
                  {notice.detail && (
                    <p className="mt-0.5 text-ink-3">{notice.detail}</p>
                  )}
                </div>
                <button
                  type="button"
                  onClick={() => {
                    setIsOpen(false);
                    onShowSection(page);
                  }}
                  className="shrink-0 cursor-pointer rounded-md bg-wash px-2 py-1 text-text transition-colors hover:bg-line focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
                >
                  {t("notices.show")}
                </button>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
};

export default NoticeChip;
