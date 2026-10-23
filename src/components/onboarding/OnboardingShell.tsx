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
 * the download, beside the button it holds back) and the primary button. 600 tall, and as wide as
 * the window from its smallest (680) to the width it opens at (720,
 * MAIN_WINDOW_SIZE), so the header and footer sit at the window's edges;
 * centred in anything bigger.
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
      <div className="relative m-auto grid h-[600px] w-full min-w-[680px] max-w-[720px] shrink-0 grid-cols-[minmax(0,1fr)] grid-rows-[56px_1fr_76px] font-ui text-body leading-[1.6] text-text antialiased [&_button:disabled]:border-line [&_button:disabled]:bg-wash [&_button:disabled]:text-ink-3 [&_button:disabled]:opacity-100">
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

const ALIGN = {
  center: "items-center text-center",
  stretch: "items-stretch text-center",
  start: "items-start text-start",
} as const;

/**
 * A screen's main block, inside the design's gutters. Given a header, it's
 * two parts: the header at the same height on every screen, and under it the
 * body, which takes the rest and centres what's in it. A screen with a lot to
 * show fills the body from just under the header; one with little sits in the
 * middle of the space left, so both look balanced and the title never moves.
 * Without a header the whole block is centred top to bottom (All set).
 */
export const ScreenMain = React.forwardRef<
  HTMLDivElement,
  React.HTMLAttributes<HTMLDivElement> & {
    /** Model's full list: 40px gutters (600 to 640 wide). */
    wide?: boolean;
    /** The column: centred (default), full width, or from the start edge. */
    align?: keyof typeof ALIGN;
    /** A ScreenHeader, pinned above the body. */
    header?: React.ReactNode;
  }
>(
  (
    {
      children,
      wide = false,
      align = "center",
      header,
      className = "",
      ...rest
    },
    ref,
  ) => (
    <div
      ref={ref}
      {...rest}
      className={`flex h-full min-h-0 min-w-0 flex-col ${header ? "pt-[16px] pb-[8px]" : "justify-center-safe py-[8px]"} ${ALIGN[align]} ${wide ? "px-gutter-wide" : "px-gutter"} ${className}`}
    >
      {header}
      {header ? (
        <div
          className={`flex min-h-0 w-full flex-1 flex-col justify-center-safe pt-lead ${ALIGN[align]}`}
        >
          {children}
        </div>
      ) : (
        children
      )}
    </div>
  ),
);
ScreenMain.displayName = "ScreenMain";

/**
 * A screen's title and the line under it, the same on every screen. Given to
 * ScreenMain as its header, the title lines up with every other screen's.
 */
export const ScreenHeader: React.FC<{
  title: React.ReactNode;
  sub?: React.ReactNode;
  /** All set's 40px finish line. */
  hero?: boolean;
  subClassName?: string;
  /** A note over the title (Permissions' welcome back), kept out of the
   *  flow so it doesn't push the title down. */
  above?: React.ReactNode;
}> = ({ title, sub, hero = false, subClassName, above }) => (
  <div className="relative w-full flex-none">
    {above && (
      <div className="absolute inset-x-0 bottom-full mb-[12px]">{above}</div>
    )}
    <ScreenTitle hero={hero}>{title}</ScreenTitle>
    {sub && <ScreenSub className={subClassName}>{sub}</ScreenSub>}
  </div>
);

/** A screen title: 26px display, or 40px for a finish line (All set). */
export const ScreenTitle: React.FC<{
  children: React.ReactNode;
  hero?: boolean;
  className?: string;
}> = ({ children, hero = false, className = "" }) => (
  <h1
    className={`m-0 font-display leading-[1.15] tracking-[-0.015em] text-balance ${hero ? "text-hero font-[750]" : "text-title font-[650]"} ${className}`}
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

/**
 * The space between a screen's header and its content: 20px when the screen
 * is full, growing to 32px when there's room.
 */
export const ScreenGap: React.FC = () => (
  <div aria-hidden="true" className="min-h-lead max-h-lead-max flex-[1_1_0]" />
);
