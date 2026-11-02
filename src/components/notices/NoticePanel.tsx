import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { X } from "lucide-react";
import type { NoticeLevel } from "@/bindings";
import { NoticeBadgeIcon } from "@/components/icons/NoticeBadgeIcon";
import { noticeDisplay } from "@/lib/notices";
import { type LocalNotice, useNoticeStore } from "@/stores/noticeStore";

// Longer than a usual toast: a notice can carry a raw error worth reading.
const TOAST_MS = 8000;

interface NoticeRowProps {
  level: NoticeLevel;
  title: string;
  explanation?: string;
  detail?: string | null;
  action?: LocalNotice["action"];
  onDismiss: () => void;
}

const NoticeRow: React.FC<NoticeRowProps> = ({
  level,
  title,
  explanation,
  detail,
  action,
  onDismiss,
}) => {
  const { t } = useTranslation();
  const color = level === "error" ? "text-error" : "text-warning";

  return (
    <div className="flex items-start gap-2.5 px-3 py-2.5">
      <NoticeBadgeIcon className={`mt-[3px] h-3.5 w-3.5 shrink-0 ${color}`} />
      <div className="min-w-0 flex-1 select-text break-words">
        <p className="text-sm font-medium text-text">{title}</p>
        {explanation && <p className="mt-0.5">{explanation}</p>}
        {detail && <p className="mt-0.5 text-ink-3">{detail}</p>}
        {action && (
          <button
            type="button"
            onClick={action.onClick}
            className="mt-1 cursor-pointer rounded-sm text-text underline underline-offset-2 focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
          >
            {action.label}
          </button>
        )}
      </div>
      <button
        type="button"
        aria-label={t("common.close")}
        onClick={onDismiss}
        className="-me-1 -mt-0.5 shrink-0 cursor-pointer rounded-md p-1 text-ink-3 transition-colors hover:text-text focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
      >
        <X className="h-3.5 w-3.5" />
      </button>
    </div>
  );
};

const NoticeCard: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const toasts = useNoticeStore((s) => s.toasts);
  const panelOpen = useNoticeStore((s) => s.panelOpen);
  const footerShown = useNoticeStore((s) => s.footerShown);
  const hide = useNoticeStore((s) => s.hide);
  const [hovered, setHovered] = useState(false);

  // `toasts` changes with every new notice, which restarts the time.
  useEffect(() => {
    if (panelOpen || hovered) return;
    const timer = setTimeout(hide, TOAST_MS);
    return () => clearTimeout(timer);
  }, [panelOpen, hovered, toasts, hide]);

  // Only the open list closes this way. A toast ends by itself, and dialogs
  // and shortcut capture have their own use for Escape.
  useEffect(() => {
    if (!panelOpen) return;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") hide();
    };
    const handleClickOutside = (event: MouseEvent) => {
      if (!(event.target as Element).closest("[data-notices]")) hide();
    };
    document.addEventListener("keydown", handleKeyDown);
    document.addEventListener("mousedown", handleClickOutside);
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
      document.removeEventListener("mousedown", handleClickOutside);
    };
  }, [panelOpen, hide]);

  // Over dialogs. Onboarding has no footer, and its Continue button is where
  // the card would be, so there it sits below the window's top bar.
  const position = footerShown ? "bottom-9" : "top-14";

  return (
    <div
      data-notices
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      className={`fixed ${position} end-4 z-[60] max-h-[60vh] w-80 divide-y divide-line overflow-y-auto rounded-lg border border-line bg-background text-xs text-ink-2 shadow-lg motion-safe:animate-rise`}
    >
      {children}
    </div>
  );
};

/**
 * The only place messages are drawn. It's mounted outside the app's screens, so
 * it also works during onboarding, and a toast opens into the panel in place:
 * they're this one card, showing some notices or all of them.
 */
export const NoticePanel: React.FC = () => {
  const { t } = useTranslation();
  const notices = useNoticeStore((s) => s.notices);
  const local = useNoticeStore((s) => s.local);
  const toasts = useNoticeStore((s) => s.toasts);
  const panelOpen = useNoticeStore((s) => s.panelOpen);
  const dismiss = useNoticeStore((s) => s.dismiss);
  const clearLocal = useNoticeStore((s) => s.clearLocal);

  const isShown = (id: string) => panelOpen || toasts.includes(id);
  const rows: (NoticeRowProps & { id: string })[] = [
    ...local
      .filter((notice) => isShown(notice.id))
      .map(({ id, level, title, detail, action }) => ({
        id,
        level,
        title,
        detail,
        action,
        onDismiss: () => clearLocal(id),
      })),
    ...notices
      .filter((notice) => isShown(notice.source))
      .map((notice) => {
        const { title, explanation } = noticeDisplay(notice.code);
        return {
          id: notice.source,
          level: notice.level,
          title: t(title),
          explanation: explanation?.(t),
          detail: notice.detail,
          onDismiss: () => dismiss(notice),
        };
      }),
  ];

  return (
    <>
      {/* Always mounted: a screen reader only announces changes to a live
          region that was already there. New toasts only: the open list is
          content to move through, not to read out. */}
      <div role="status" aria-atomic="true" className="sr-only">
        {rows
          .filter(({ id }) => toasts.includes(id))
          .flatMap(({ title, explanation, detail }) => [
            title,
            explanation,
            detail,
          ])
          .filter(Boolean)
          .map((line, index) => (
            <p key={index}>{line}</p>
          ))}
      </div>
      {rows.length > 0 && (
        <NoticeCard>
          {rows.map(({ id, ...row }) => (
            <NoticeRow key={id} {...row} />
          ))}
        </NoticeCard>
      )}
    </>
  );
};
