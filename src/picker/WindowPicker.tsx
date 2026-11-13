import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type PickerHover } from "@/bindings";
import "./WindowPicker.css";

// One panel of the window picker: highlights the window under the pointer,
// a click picks it, Esc gives up. Each screen has its own panel, so the
// highlight follows the pointer only while it's over this one.
const sameWindow = (a?: PickerHover, b?: PickerHover) =>
  a?.app_name === b?.app_name &&
  a?.x === b?.x &&
  a?.y === b?.y &&
  a?.width === b?.width &&
  a?.height === b?.height;

export default function WindowPicker() {
  const { t } = useTranslation();
  // The windows under the pointer, front to back, and which of them is
  // highlighted: Tab goes one deeper, to a window another one covers.
  const [stack, setStack] = useState<PickerHover[]>([]);
  const [depth, setDepth] = useState(0);
  const depthRef = useRef(0);
  depthRef.current = depth;
  // A click that found no window to pick; the hint says so for a moment.
  const [missed, setMissed] = useState(false);
  // The latest pointer position and whether a lookup is in flight: pointer
  // events come faster than the lookup answers, so only the newest is asked.
  const pointer = useRef<{ x: number; y: number } | null>(null);
  const looking = useRef(false);

  useEffect(() => {
    const lookUp = async () => {
      if (looking.current) return;
      looking.current = true;
      try {
        let asked = pointer.current;
        while (asked) {
          const found = await commands.pickerHover(asked.x, asked.y);
          setStack((previous) => {
            // A new front window means a new stack; the depth starts over.
            if (!sameWindow(previous[0], found[0])) setDepth(0);
            return found;
          });
          asked = pointer.current !== asked ? pointer.current : null;
        }
      } finally {
        looking.current = false;
      }
    };
    const onMove = (e: MouseEvent) => {
      pointer.current = { x: e.clientX, y: e.clientY };
      void lookUp();
    };
    const onLeave = () => {
      pointer.current = null;
      setStack([]);
    };
    let missedTimer: ReturnType<typeof setTimeout> | undefined;
    const onClick = async (e: MouseEvent) => {
      if (await commands.pickerPick(e.clientX, e.clientY, depthRef.current))
        return;
      setMissed(true);
      clearTimeout(missedTimer);
      missedTimer = setTimeout(() => setMissed(false), 2000);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") void commands.pickerCancel();
      if (e.key === "Tab") {
        e.preventDefault();
        setStack((current) => {
          if (current.length > 1)
            setDepth(
              (d) =>
                (d + (e.shiftKey ? -1 : 1) + current.length) % current.length,
            );
          return current;
        });
      }
    };
    window.addEventListener("mousemove", onMove);
    // mouseleave is sent to elements, not the window: the document gets it
    // when the pointer leaves this screen for another.
    document.addEventListener("mouseleave", onLeave);
    const onClickEvent = (e: MouseEvent) => void onClick(e);
    window.addEventListener("click", onClickEvent);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseleave", onLeave);
      window.removeEventListener("click", onClickEvent);
      clearTimeout(missedTimer);
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  const hover = stack[Math.min(depth, stack.length - 1)];
  return (
    <div className="picker">
      {hover && (
        <div
          className="picker-highlight"
          style={{
            left: hover.x,
            top: hover.y,
            width: hover.width,
            height: hover.height,
          }}
        >
          <span className="picker-label">{hover.app_name}</span>
        </div>
      )}
      <div
        className={missed ? "picker-hint picker-hint-missed" : "picker-hint"}
      >
        {t(missed ? "picker.noWindow" : "picker.hint")}
      </div>
    </div>
  );
}
