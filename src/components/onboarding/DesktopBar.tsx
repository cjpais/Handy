import React from "react";
import type { OSType } from "@/lib/utils/keyboard";
import TrayIcon from "../icons/TrayIcon";
import "./desktopBar.css";

// Demo draws the bar small, on the laptop; Try it close up. The menu bar's
// menus, as blanks, at each size.
const MENUS = {
  demo: ["w-[27.5px]", "w-[20px]", "w-[22.5px]"],
  try: ["w-[30px]", "w-[22px]", "w-[26px]"],
};
const ICON_SIZE = { demo: 15, try: 19 };

/** Tux, flat like the other marks: the belly and eyes are cut out. */
const Tux = () => (
  <svg className="desktop-tux" viewBox="0 0 16 16">
    <path
      fillRule="evenodd"
      d="M8 .8C10.1.8 11.3 2.5 11.3 4.6c0 1 .1 1.7.6 2.5 1.1 1.7 2 3.6 1.4 5.6-.4 1.4-2.5 2.2-5.3 2.2s-4.9-.8-5.3-2.2c-.6-2 .3-3.9 1.4-5.6.5-.8.6-1.5.6-2.5C4.7 2.5 5.9.8 8 .8ZM5 10.8a3 3.5 0 1 0 6 0 3 3.5 0 1 0-6 0Zm1.15-6.7a.75.75 0 1 0 1.5 0 .75.75 0 1 0-1.5 0Zm2.2 0a.75.75 0 1 0 1.5 0 .75.75 0 1 0-1.5 0Z"
    />
    <ellipse cx="5.3" cy="14.9" rx="2.3" ry="1" />
    <ellipse cx="10.7" cy="14.9" rx="2.3" ry="1" />
  </svg>
);

/**
 * The menu bar (macOS) or the taskbar (Windows, Linux) on Demo's and Try
 * it's drawn screens, where Handy's icon lives.
 */
export const DesktopBar: React.FC<{
  os: OSType;
  size: "demo" | "try";
  /** Handy's icon, as the real one changes: an ear while it listens, a
   *  brain while it transcribes. */
  state: React.ComponentProps<typeof TrayIcon>["state"];
  /** Handy's icon: Try it's glove flies into it. */
  trayRef?: React.Ref<HTMLSpanElement>;
  /** The icon just caught the glove. */
  caught?: boolean;
}> = ({ os, size, state, trayRef, caught }) => {
  const tray = (
    <span
      ref={trayRef}
      className="desktop-tray"
      data-caught={caught || undefined}
    >
      <TrayIcon state={state} size={ICON_SIZE[size]} />
    </span>
  );
  const clock = <span className="tabular-nums">9:41</span>;
  const scale = size === "demo" ? "is-demo" : "is-try";
  return os === "macos" ? (
    <div className={`desktop-bar is-menu ${scale}`} aria-hidden="true">
      <span className="desktop-apple" />
      {MENUS[size].map((width) => (
        <i key={width} className={width} />
      ))}
      <span className="flex-1" />
      {tray}
      {clock}
    </div>
  ) : (
    <div className={`desktop-bar is-task ${scale}`} aria-hidden="true">
      <span className="flex-1" />
      {os === "linux" ? (
        <Tux />
      ) : (
        <span className="desktop-start">
          {[0, 1, 2, 3].map((i) => (
            <i key={i} />
          ))}
        </span>
      )}
      {[0, 1, 2].map((i) => (
        <b key={i} />
      ))}
      <span className="flex-1" />
      {tray}
      {clock}
    </div>
  );
};
