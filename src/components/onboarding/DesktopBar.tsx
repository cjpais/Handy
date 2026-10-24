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

/**
 * The menu bar (macOS) or the taskbar (Windows) on Demo's and Try it's drawn
 * screens, where Handy's icon lives.
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
      <span className="desktop-start">
        {[0, 1, 2, 3].map((i) => (
          <i key={i} />
        ))}
      </span>
      {[0, 1, 2].map((i) => (
        <b key={i} />
      ))}
      <span className="flex-1" />
      {tray}
      {clock}
    </div>
  );
};
