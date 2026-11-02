import React, { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { NoticeBadgeIcon } from "@/components/icons/NoticeBadgeIcon";
import { useNoticeStore } from "@/stores/noticeStore";

/** Counts the active notices and opens the panel that lists them. */
const NoticeChip: React.FC = () => {
  const { t } = useTranslation();
  const backendNotices = useNoticeStore((s) => s.notices);
  const local = useNoticeStore((s) => s.local);
  const panelOpen = useNoticeStore((s) => s.panelOpen);
  const openPanel = useNoticeStore((s) => s.openPanel);
  const hide = useNoticeStore((s) => s.hide);
  const setFooterShown = useNoticeStore((s) => s.setFooterShown);

  useEffect(() => {
    setFooterShown(true);
    return () => setFooterShown(false);
  }, [setFooterShown]);

  const notices = [...backendNotices, ...local.filter((n) => n.kept)];
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
    <button
      type="button"
      data-notices
      aria-expanded={panelOpen}
      onClick={panelOpen ? hide : openPanel}
      className="flex shrink-0 cursor-pointer items-center gap-1.5 whitespace-nowrap rounded-sm text-ink-2 transition-colors hover:text-text aria-expanded:text-text focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
    >
      <NoticeBadgeIcon
        className={`h-3 w-3 ${errors > 0 ? "text-error" : "text-warning"}`}
      />
      {chipText}
    </button>
  );
};

export default NoticeChip;
