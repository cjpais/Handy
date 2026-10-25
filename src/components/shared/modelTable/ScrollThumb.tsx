import React, { useEffect, useLayoutEffect, useRef } from "react";

/** The app's scrollbar (App.css): a 7px pill, 3px in, at least 32px tall. */
const INSET = 3;
const MIN_HEIGHT = 32;
/** How long it stays after the scrolling stops. */
const LINGER_MS = 800;

/**
 * A list's scrollbar drawn over it instead of beside it, so its rows and
 * their dividers run the full width whatever the platform's scrollbars do.
 * It looks like the app's own (--scrollbar-thumb in a 14px strip) and, as
 * macOS's do, shows while scrolling or with the pointer over the list; it
 * can be dragged. The list hides its native one. Goes after the list, in a
 * `relative group/scroll` box around it.
 *
 * Solid, not see-through as the native one is: over a picked row or a
 * divider a see-through thumb mixes into a third grey. Its colours are what
 * --scrollbar-thumb (15% text) and its hover (30%) look like on the page.
 */
export const ScrollThumb: React.FC<{
  listRef: React.RefObject<HTMLElement | null>;
}> = ({ listRef }) => {
  const thumbRef = useRef<HTMLDivElement>(null);
  const drag = useRef<{ y: number; top: number } | null>(null);
  const linger = useRef(0);

  /** Its length is how much of the list shows; its place, how far down. */
  const place = useRef(() => {});
  place.current = () => {
    const list = listRef.current;
    const thumb = thumbRef.current;
    if (!list || !thumb) return;
    const { scrollTop, scrollHeight, clientHeight } = list;
    const max = scrollHeight - clientHeight;
    thumb.hidden = max <= 1;
    if (thumb.hidden) return;
    const track = clientHeight - INSET * 2;
    const height = Math.max(MIN_HEIGHT, (track * clientHeight) / scrollHeight);
    // Past either end (a bounce), it stays at that end.
    const at = Math.min(1, Math.max(0, scrollTop / max));
    thumb.style.height = `${height}px`;
    thumb.style.transform = `translateY(${INSET + at * (track - height)}px)`;
  };

  const show = () => {
    const thumb = thumbRef.current;
    if (!thumb) return;
    thumb.dataset.active = "";
    clearTimeout(linger.current);
    linger.current = window.setTimeout(() => {
      if (!drag.current) delete thumb.dataset.active;
    }, LINGER_MS);
  };

  // Every render: a search or a filter changes how much there is.
  useLayoutEffect(() => place.current());
  useEffect(() => {
    const list = listRef.current;
    if (!list) return;
    const onScroll = () => {
      place.current();
      show();
    };
    list.addEventListener("scroll", onScroll, { passive: true });
    const resized = new ResizeObserver(() => place.current());
    resized.observe(list);
    return () => {
      list.removeEventListener("scroll", onScroll);
      resized.disconnect();
      clearTimeout(linger.current);
    };
  }, [listRef]);

  const release = () => {
    drag.current = null;
    thumbRef.current?.removeAttribute("data-dragging");
    show();
  };

  return (
    <div
      aria-hidden="true"
      className="pointer-events-none absolute inset-y-0 end-0 w-[14px]"
    >
      <div
        ref={thumbRef}
        onPointerDown={(e) => {
          const list = listRef.current;
          if (!list) return;
          e.preventDefault();
          e.currentTarget.setPointerCapture(e.pointerId);
          drag.current = { y: e.clientY, top: list.scrollTop };
          e.currentTarget.dataset.dragging = "";
          show();
        }}
        onPointerMove={(e) => {
          const list = listRef.current;
          const from = drag.current;
          if (!list || !from) return;
          // The thumb's travel stands for the list's.
          const travel =
            list.clientHeight - INSET * 2 - e.currentTarget.offsetHeight;
          const max = list.scrollHeight - list.clientHeight;
          if (travel > 0)
            list.scrollTop = from.top + ((e.clientY - from.y) * max) / travel;
        }}
        onPointerUp={release}
        onLostPointerCapture={release}
        className="pointer-events-auto absolute top-0 start-[3px] end-[4px] rounded-full bg-[color-mix(in_srgb,var(--color-text)_15%,var(--color-background))] opacity-0 transition-[opacity,background-color] duration-300 hover:bg-[color-mix(in_srgb,var(--color-text)_30%,var(--color-background))] group-hover/scroll:opacity-100 data-active:opacity-100 data-dragging:bg-[color-mix(in_srgb,var(--color-text)_30%,var(--color-background))]"
      />
    </div>
  );
};
