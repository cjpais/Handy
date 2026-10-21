import React from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../ui/Button";

interface OnboardingShellProps {
  /** Where this screen is in the flow; no dashes on a standalone screen. */
  progress?: { index: number; count: number };
  /** Top right: the download pill, from Model on. */
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
 * The window every onboarding screen draws in: progress dashes and the
 * download pill on top, the screen, then a footer with Back (or the screen's
 * own left slot), a hint and the primary button. 680 × 570, the window's
 * own size, centred if the window grows.
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
          button is the one to press next. */}
      <div className="relative m-auto grid h-[570px] w-[680px] shrink-0 grid-rows-[56px_1fr_76px] font-ui text-body leading-[1.6] text-text antialiased [&_button:disabled]:border-line [&_button:disabled]:bg-wash [&_button:disabled]:text-ink-3 [&_button:disabled]:opacity-100">
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
                        : "w-[14px] bg-line-strong"
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
              <Button
                variant="ghost"
                size="lg"
                onClick={onBack}
                className="-ms-[12px] !px-[12px]"
              >
                {t("onboarding.shell.back")}
              </Button>
            ) : (
              <span />
            ))}
          <span
            className={`ms-auto text-small ${hintTone === "warning" ? "text-warning" : "text-ink-3"}`}
          >
            {hint}
          </span>
          {primary}
        </div>
      </div>
    </div>
  );
};

/** A screen's main block: the design's 64px gutters, centred column. */
export const ScreenMain: React.FC<{
  children: React.ReactNode;
  /** Task screens start the title 46px from the window's top. */
  task?: boolean;
  /** Model's full list: 40px gutters (600 wide). */
  wide?: boolean;
  className?: string;
}> = ({ children, task = false, wide = false, className = "" }) => (
  <div
    className={`flex h-full min-h-0 flex-col items-center text-center ${wide ? "px-gutter-wide" : "px-gutter"} ${task ? "justify-start pt-[4px]" : "justify-center"} ${className}`}
  >
    {children}
  </div>
);

/** A screen title (26px display). On task screens it rides up 14px. */
export const ScreenTitle: React.FC<{
  children: React.ReactNode;
  task?: boolean;
  className?: string;
}> = ({ children, task = false, className = "" }) => (
  <h1
    className={`m-0 font-display text-title font-[650] leading-[1.15] tracking-[-0.015em] text-balance ${task ? "-mt-[14px]" : ""} ${className}`}
  >
    {children}
  </h1>
);

/** The line under a title: 15px secondary text, at most 440 wide. */
export const ScreenSub: React.FC<{
  children: React.ReactNode;
  className?: string;
}> = ({ children, className = "" }) => (
  <p
    className={`mt-[10px] mb-0 max-w-[440px] text-ink-2 text-pretty ${className}`}
  >
    {children}
  </p>
);
