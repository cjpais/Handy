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

const FloatingButton: React.FC = () => {
  const { t } = useTranslation();
  const [state, setState] = useState<ButtonState>("idle");
  const [pressed, setPressed] = useState(false);
  const drag = useRef<{ startY: number; dragging: boolean } | null>(null);

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
    drag.current = { startY: e.screenY, dragging: false };
    setPressed(true);
  };

  const onPointerMove = (e: React.PointerEvent<HTMLButtonElement>) => {
    const current = drag.current;
    if (!current) return;
    const deltaY = e.screenY - current.startY;
    if (!current.dragging && Math.abs(deltaY) < DRAG_THRESHOLD) return;
    current.dragging = true;
    commands.floatingButtonDragged(deltaY, false);
  };

  const onPointerUp = (e: React.PointerEvent<HTMLButtonElement>) => {
    const current = drag.current;
    drag.current = null;
    setPressed(false);
    if (!current) return;
    if (current.dragging) {
      commands.floatingButtonDragged(e.screenY - current.startY, true);
    } else {
      commands.floatingButtonPressed();
    }
  };

  const onPointerCancel = () => {
    drag.current = null;
    setPressed(false);
  };

  return (
    <button
      type="button"
      className={`floating-button ${state}${pressed ? " pressed" : ""}`}
      aria-label={t("settings.advanced.showFloatingButton.buttonLabel")}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerCancel}
      onContextMenu={(e) => e.preventDefault()}
    >
      <Mic size={20} strokeWidth={2.2} />
    </button>
  );
};

export default FloatingButton;
