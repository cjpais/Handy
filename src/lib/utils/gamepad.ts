/**
 * Gamepad utility functions for formatting and normalizing gamepad buttons and combinations
 */

export const STANDARD_GAMEPAD_BUTTON_MAP: Record<number, string> = {
  0: "a",
  1: "b",
  2: "x",
  3: "y",
  4: "lb",
  5: "rb",
  6: "lt",
  7: "rt",
  8: "select",
  9: "start",
  10: "ls",
  11: "rs",
  12: "dpad_up",
  13: "dpad_down",
  14: "dpad_left",
  15: "dpad_right",
  16: "guide",
};

export const BUTTON_ORDER: Record<string, number> = {
  select: 1,
  start: 2,
  guide: 3,
  lb: 4,
  rb: 5,
  lt: 6,
  rt: 7,
  dpad_up: 8,
  dpad_down: 9,
  dpad_left: 10,
  dpad_right: 11,
  a: 12,
  b: 13,
  x: 14,
  y: 15,
  ls: 16,
  rs: 17,
};

/**
 * Format a single gamepad button for display.
 */
export const formatGamepadPart = (part: string): string => {
  const trimmed = part.trim().toLowerCase();
  switch (trimmed) {
    case "select":
    case "back":
    case "view":
    case "share":
      return "Select";
    case "start":
    case "menu":
    case "options":
      return "Start";
    case "a":
    case "south":
    case "cross":
      return "A";
    case "b":
    case "east":
    case "circle":
      return "B";
    case "x":
    case "west":
    case "square":
      return "X";
    case "y":
    case "north":
    case "triangle":
      return "Y";
    case "lb":
    case "l1":
    case "left_shoulder":
    case "left_bumper":
      return "LB";
    case "rb":
    case "r1":
    case "right_shoulder":
    case "right_bumper":
      return "RB";
    case "lt":
    case "l2":
    case "left_trigger":
      return "LT";
    case "rt":
    case "r2":
    case "right_trigger":
      return "RT";
    case "ls":
    case "l3":
    case "left_stick":
    case "left_thumb":
      return "LS";
    case "rs":
    case "r3":
    case "right_stick":
    case "right_thumb":
      return "RS";
    case "dpad_up":
    case "up":
      return "D-Pad Up";
    case "dpad_down":
    case "down":
      return "D-Pad Down";
    case "dpad_left":
    case "left":
      return "D-Pad Left";
    case "dpad_right":
    case "right":
      return "D-Pad Right";
    case "guide":
    case "mode":
    case "home":
      return "Guide";
    default:
      return trimmed.toUpperCase();
  }
};

/**
 * Format a gamepad combination string like "select + start" or "select+start"
 * into a human-readable display string like "Select + Start".
 */
export const formatGamepadCombination = (combination: string): string => {
  if (!combination) return "Select + Start";
  const parts = combination
    .split(/[+,\s]/)
    .map((p) => p.trim())
    .filter(Boolean);

  if (parts.length === 0) return "Select + Start";

  return parts.map(formatGamepadPart).join(" + ");
};

/**
 * Sort buttons in a canonical chord order (e.g. select before start)
 */
export const sortGamepadButtons = (buttons: string[]): string[] => {
  return [...buttons].sort((a, b) => {
    const orderA = BUTTON_ORDER[a.toLowerCase()] ?? 99;
    const orderB = BUTTON_ORDER[b.toLowerCase()] ?? 99;
    return orderA - orderB;
  });
};
