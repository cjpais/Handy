import { useEffect, useRef, type RefObject } from "react";

/**
 * While `active`, a press anywhere outside `inside` calls `onAway` with its
 * target, before the press reaches anything else.
 */
export const usePressAway = (
  active: boolean,
  inside: RefObject<HTMLElement | null>,
  onAway: (target: Node) => void,
) => {
  const away = useRef(onAway);
  away.current = onAway;
  useEffect(() => {
    if (!active) return;
    const onDown = (ev: PointerEvent) => {
      const target = ev.target as Node;
      if (!inside.current?.contains(target)) away.current(target);
    };
    document.addEventListener("pointerdown", onDown, true);
    return () => document.removeEventListener("pointerdown", onDown, true);
  }, [active, inside]);
};
