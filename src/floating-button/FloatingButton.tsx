import { listen } from "@tauri-apps/api/event";
import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Mic } from "lucide-react";
import { commands } from "@/bindings";
import { syncLanguageFromSettings } from "@/i18n";
import "./FloatingButton.css";

type ButtonState = "idle" | "recording" | "transcribing";

// Vertical movement (CSS px) before a press counts as a drag instead of a tap.
const DRAG_THRESHOLD = 8;

interface Press {
  /** Where inside the window the pointer went down (CSS px from its top). */
  grabY: number;
  dragging: boolean;
  /** A move is in flight; pointer events are ignored until it returns. */
  moving: boolean;
  /** `performance.now()` when the last move returned. */
  movedAt: number;
}

const FloatingButton: React.FC = () => {
  const { t } = useTranslation();
  const [state, setState] = useState<ButtonState>("idle");
  const [pressed, setPressed] = useState(false);
  const press = useRef<Press | null>(null);

  useEffect(() => {
    syncLanguageFromSettings();
    const unlisten = listen<ButtonState>("floating-button-state", (event) =>
      setState(event.payload),
    );
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const onPointerDown = (e: React.PointerEvent<HTMLButtonElement>) => {
    e.currentTarget.setPointerCapture(e.pointerId);
    press.current = {
      grabY: e.clientY,
      dragging: false,
      moving: false,
      movedAt: 0,
    };
    setPressed(true);
  };

  // The window moves under the pointer, so the drag works from `clientY` — the
  // pointer's position *inside* the window — and keeps it on the grab point.
  // Screen coordinates are unusable here: for touch input they are derived
  // from a cached window position that lags behind the moves, which made the
  // button jitter back and forth.
  const onPointerMove = async (e: React.PointerEvent<HTMLButtonElement>) => {
    const current = press.current;
    if (!current) return;
    const drift = e.clientY - current.grabY;
    if (!current.dragging) {
      if (Math.abs(drift) < DRAG_THRESHOLD) return;
      current.dragging = true;
    }
    // One move at a time, and skip events measured before the last move
    // landed: they are relative to the window's previous position.
    if (current.moving || e.timeStamp <= current.movedAt) return;
    if (Math.round(drift) === 0) return;
    current.moving = true;
    await commands.floatingButtonDragBy(drift, false);
    current.movedAt = performance.now();
    current.moving = false;
  };

  const endPress = (tap: boolean) => {
    const current = press.current;
    press.current = null;
    setPressed(false);
    if (!current) return;
    if (current.dragging) {
      commands.floatingButtonDragBy(0, true);
    } else if (tap) {
      commands.floatingButtonPressed();
    }
  };

  return (
    <button
      type="button"
      className={`floating-button ${state}${pressed ? " pressed" : ""}`}
      aria-label={t("settings.advanced.showFloatingButton.buttonLabel")}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={() => endPress(true)}
      onPointerCancel={() => endPress(false)}
      onContextMenu={(e) => e.preventDefault()}
    >
      <span className="fb-badge">
        <span className="fb-icon">
          <Mic size={18} strokeWidth={2.2} />
        </span>
        <span className="fb-spinner" />
      </span>
    </button>
  );
};

export default FloatingButton;
