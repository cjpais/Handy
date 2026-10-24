import React from "react";
import { useTranslation } from "react-i18next";

interface OnboardingShellProps {
  /** Where this screen is in the flow; no dashes on a standalone screen. */
  progress?: { index: number; count: number };
  /** Top right: Welcome's menu language. */
  pill?: React.ReactNode;
  children: React.ReactNode;
  /** The footer's left slot; replaces Back. */
  left?: React.ReactNode;
  onBack?: () => void;
  hint?: React.ReactNode;
  /** Warnings read as warnings, not hints. */
  hintTone?: "quiet" | "warning";
  primary?: React.ReactNode;
}

/**
 * The window every onboarding screen draws in: progress dashes on top, the
 * screen, then a footer with Back (or the screen's own left slot), a hint (or
 * the download, beside the button it holds back) and the primary button.
 * 720 by 600, the window's smallest and opening size (MAIN_WINDOW_SIZE), so
 * the header and footer sit at the window's edges; centred in anything
 * bigger.
 */
export const OnboardingShell: React.FC<OnboardingShellProps> = ({
  progress,
  pill,
  children,
  left,
  onBack,
  hint,
  hintTone = "quiet",
  primary,
}) => {
  const { t } = useTranslation();
  return (
    <div
      data-onboarding=""
      className="h-screen w-full flex overflow-hidden bg-background select-none cursor-default"
    >
      {/* A disabled button goes neutral, not faded pink: a screen's only pink
          button is the one to press next. The one column is capped at the
          window's width, so a long row truncates instead of widening it. */}
      <div className="relative m-auto grid h-[600px] w-[720px] shrink-0 grid-cols-[minmax(0,1fr)] grid-rows-[48px_1fr_76px] font-ui text-body leading-[1.6] text-text antialiased [&_button:disabled]:border-line [&_button:disabled]:bg-wash [&_button:disabled]:text-ink-3 [&_button:disabled]:opacity-100">
        <div className="relative flex items-center justify-between ps-[28px] pe-[20px]">
          {progress ? (
            <div
              className="flex gap-[6px]"
              role="img"
              aria-label={t("onboarding.shell.progress", {
                step: progress.index + 1,
                count: progress.count,
              })}
            >
              {Array.from({ length: progress.count }, (_, i) => (
                <i
                  key={i}
                  className={`h-[4px] rounded-[2px] transition-[width,background-color] duration-300 ease-glide ${
                    i === progress.index
                      ? "w-[28px] bg-background-ui"
                      : i < progress.index
                        ? "w-[14px] bg-ink-3"
                        : "w-[14px] bg-line"
                  }`}
                />
              ))}
            </div>
          ) : (
            <span />
          )}
          {pill}
        </div>
        <div className="min-h-0">{children}</div>
        <div className="flex items-center justify-between gap-[16px] px-[28px] pb-[4px]">
          {left ??
            (onBack ? (
              // Quiet next to the primary action: secondary ink, and on hover
              // the full text on the wash, like the screens' own rows.
              <button
                type="button"
                onClick={onBack}
                className="-ms-[12px] cursor-pointer rounded-lg border border-transparent px-[12px] py-[7.5px] text-body font-medium text-ink-2 transition-colors hover:bg-wash hover:text-text focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
              >
                {t("onboarding.shell.back")}
              </button>
            ) : (
              <span />
            ))}
          <div
            className={`ms-auto min-w-0 text-small ${hintTone === "warning" ? "text-warning" : "text-ink-3"}`}
          >
            {hint}
          </div>
          {primary}
        </div>
      </div>
    </div>
  );
};

/** The column: how the content lines up across. Text is always centred. */
const ALIGN = {
  center: "items-center",
  stretch: "items-stretch",
} as const;

/** Between the header and the content, per layout. */
const GAP = {
  /* 20px when the screen is full, growing to 32px when there's room. */
  center: "min-h-lead max-h-lead-max flex-[1_1_0]",
  top: "h-lead flex-none",
} as const;

/**
 * A screen's main block, inside the design's gutters: the header, a gap, and
 * the content. Both layouts share one top line, the highest a title sits, so
 * there's no padding above; the 8px below keeps content off the footer.
 */
export const ScreenMain = React.forwardRef<
  HTMLDivElement,
  React.HTMLAttributes<HTMLDivElement> & {
    /** Model's full list: 40px gutters (640 wide). */
    wide?: boolean;
    /** The column: centred (default) or full width. */
    align?: keyof typeof ALIGN;
    /** The title block, usually a ScreenHeader. */
    header?: React.ReactNode;
    /**
     * "center" (default): header, gap and content centred together, on the
     * top line once they fill the height; for content that fits. "top": the
     * header on the top line and the content 20px under it, with the rest of
     * the height to fill; for content that wants the room.
     */
    layout?: keyof typeof GAP;
  }
>(
  (
    {
      children,
      wide = false,
      align = "center",
      header,
      layout = "center",
      className = "",
      ...rest
    },
    ref,
  ) => (
    <div
      ref={ref}
      {...rest}
      className={`flex h-full min-h-0 min-w-0 flex-col ${layout === "center" ? "justify-center-safe" : ""} text-center ${wide ? "px-gutter-wide" : "px-gutter"} ${className}`}
    >
      {header}
      {header && <div aria-hidden="true" className={GAP[layout]} />}
      <div
        className={`flex min-h-0 w-full flex-col ${layout === "top" ? "flex-1" : ""} ${ALIGN[align]}`}
      >
        {children}
      </div>
    </div>
  ),
);
ScreenMain.displayName = "ScreenMain";

/**
 * A screen's title and the line under it, the same on every screen. Given to
 * ScreenMain as its header, the spacing around it is the same everywhere.
 */
export const ScreenHeader: React.FC<{
  title: React.ReactNode;
  sub?: React.ReactNode;
  subClassName?: string;
  /** A note over the title (Permissions' welcome back), kept out of the
   *  flow so it doesn't push the title down. */
  above?: React.ReactNode;
}> = ({ title, sub, subClassName, above }) => (
  <div className="relative w-full flex-none">
    {above && (
      <div className="absolute inset-x-0 bottom-full mb-[12px]">{above}</div>
    )}
    <ScreenTitle>{title}</ScreenTitle>
    {sub && <ScreenSub className={subClassName}>{sub}</ScreenSub>}
  </div>
);

/** A screen title: 26px display. */
export const ScreenTitle: React.FC<{
  children: React.ReactNode;
  className?: string;
}> = ({ children, className = "" }) => (
  <h1
    className={`m-0 font-display leading-[1.15] tracking-[-0.015em] text-balance text-title font-[650] ${className}`}
  >
    {children}
  </h1>
);

/** The line under a title: 15px secondary text, 4px down. */
export const ScreenSub: React.FC<{
  children: React.ReactNode;
  className?: string;
}> = ({ children, className = "" }) => (
  <p className={`mt-[4px] mb-0 text-ink-2 text-pretty ${className}`}>
    {children}
  </p>
);
