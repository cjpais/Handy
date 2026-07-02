/**
 * Shared math for anchoring a floating element (tooltip, dropdown, etc.) to a
 * target element while keeping it fully inside the viewport.
 */

export const VIEWPORT_PADDING = 12;

export type VerticalPosition = "top" | "bottom";

export interface VerticalPositionResult {
  top: number;
  actualPosition: VerticalPosition;
}

/**
 * Resolves the vertical offset for content anchored above/below a target,
 * flipping to the opposite side if there isn't enough room in the preferred
 * direction.
 */
export const resolveVerticalPosition = (
  targetRect: DOMRect,
  contentHeight: number,
  preferred: VerticalPosition,
  gap: number,
  padding: number = VIEWPORT_PADDING,
): VerticalPositionResult => {
  if (preferred === "top") {
    const spaceAbove = targetRect.top - contentHeight - gap;
    if (spaceAbove < padding) {
      return { top: targetRect.bottom + gap, actualPosition: "bottom" };
    }
    return { top: targetRect.top - gap - contentHeight, actualPosition: "top" };
  }

  const spaceBelow =
    window.innerHeight - targetRect.bottom - contentHeight - gap;
  if (spaceBelow < padding) {
    return { top: targetRect.top - gap - contentHeight, actualPosition: "top" };
  }
  return { top: targetRect.bottom + gap, actualPosition: "bottom" };
};

/** Clamps a horizontal offset so the content stays within the viewport. */
export const clampHorizontal = (
  left: number,
  contentWidth: number,
  padding: number = VIEWPORT_PADDING,
): number => {
  if (left < padding) return padding;
  if (left + contentWidth > window.innerWidth - padding) {
    return window.innerWidth - contentWidth - padding;
  }
  return left;
};
