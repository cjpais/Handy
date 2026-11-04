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

/** Frame for every onboarding screen: progress dashes, screen, footer. */
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
      {/* Disabled buttons go neutral so the only pink one is the next step.
          The column is capped so a long row truncates instead of widening. */}
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

/** Horizontal alignment; text is always centred. */
const ALIGN = {
  center: "items-center",
  stretch: "items-stretch",
} as const;

/** Gap between header and content, per layout. */
const GAP = {
  center: "min-h-lead max-h-lead-max flex-[1_1_0]",
  top: "h-lead flex-none",
} as const;

/** A screen's main block: header, gap, content. */
export const ScreenMain = React.forwardRef<
  HTMLDivElement,
  React.HTMLAttributes<HTMLDivElement> & {
    align?: keyof typeof ALIGN;
    /** Usually a ScreenHeader. */
    header?: React.ReactNode;
    /** "center" centres everything; "top" pins the header and content fills. */
    layout?: keyof typeof GAP;
  }
>(
  (
    {
      children,
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
      className={`flex h-full min-h-0 min-w-0 flex-col ${layout === "center" ? "justify-center-safe" : ""} text-center px-gutter ${className}`}
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

/** A screen's title and subtitle; pass as ScreenMain's header. */
export const ScreenHeader: React.FC<{
  title: React.ReactNode;
  sub?: React.ReactNode;
  subClassName?: string;
  /** Note above the title, out of flow so it doesn't push the title down. */
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

export const ScreenSub: React.FC<{
  children: React.ReactNode;
  className?: string;
}> = ({ children, className = "" }) => (
  <p className={`mt-[4px] mb-0 text-ink-2 text-pretty ${className}`}>
    {children}
  </p>
);
