import React from "react";

const SIZES = {
  /** Demo's laptop drawing. */
  sm: { bar: "h-[27.5px] px-[11px]", dot: "size-[9px]" },
  /** Try it: the same window, close up. */
  lg: { bar: "h-[30px] px-[14px]", dot: "size-[11px]" },
};

/** The title bar of a drawn app window: three dots, the app's name centred. */
export const WindowBar: React.FC<{
  title: React.ReactNode;
  size: keyof typeof SIZES;
}> = ({ title, size }) => (
  <div
    className={`relative flex flex-none items-center text-[13px] font-semibold text-ink-3 ${SIZES[size].bar}`}
  >
    <span className="flex gap-[6px]" aria-hidden="true">
      {[0, 1, 2].map((i) => (
        <i key={i} className={`rounded-full bg-line ${SIZES[size].dot}`} />
      ))}
    </span>
    <span className="absolute inset-0 grid place-items-center">{title}</span>
  </div>
);
