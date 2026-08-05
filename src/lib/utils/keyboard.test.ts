// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/utils/keyboard.test.ts
import assert from "node:assert";
import {
  getKeyName,
  normalizeKey,
  formatKeyCombination,
  type OSType,
} from "./keyboard";

/** Minimal KeyboardEvent stand-in for getKeyName. */
const evt = (partial: { code?: string; key?: string }): KeyboardEvent =>
  partial as KeyboardEvent;

// ---------------------------------------------------------------------------
// Compound keys must be stored without spaces so both shortcut backends can
// parse them (global-hotkey: SCROLLLOCK; handy-keys: scrolllock).
// ---------------------------------------------------------------------------
const compoundCases: Array<{ code: string; expected: string }> = [
  { code: "ScrollLock", expected: "scrolllock" },
  { code: "CapsLock", expected: "capslock" },
  { code: "NumLock", expected: "numlock" },
  { code: "PageUp", expected: "pageup" },
  { code: "PageDown", expected: "pagedown" },
  { code: "PrintScreen", expected: "printscreen" },
];

for (const { code, expected } of compoundCases) {
  assert.equal(
    getKeyName(evt({ code }), "linux"),
    expected,
    `${code} should map to "${expected}" (no spaces)`,
  );
}

// Explicit anti-regression: the exact broken strings from #1848
assert.notEqual(getKeyName(evt({ code: "ScrollLock" })), "scroll lock");
assert.notEqual(getKeyName(evt({ code: "PageUp" })), "page up");
assert.notEqual(getKeyName(evt({ code: "CapsLock" })), "caps lock");
assert.notEqual(getKeyName(evt({ code: "NumLock" })), "num lock");
assert.notEqual(getKeyName(evt({ code: "PageDown" })), "page down");
assert.notEqual(getKeyName(evt({ code: "PrintScreen" })), "print screen");

// e.key fallback for CapsLock
assert.equal(getKeyName(evt({ key: "CapsLock" })), "capslock");

// ---------------------------------------------------------------------------
// Keys that already work must keep their existing stored names so we do not
// break existing installs.
// ---------------------------------------------------------------------------
const stableCases: Array<{ code: string; expected: string; os?: OSType }> = [
  { code: "Space", expected: "space" },
  { code: "Tab", expected: "tab" },
  { code: "Enter", expected: "enter" },
  { code: "Escape", expected: "esc" },
  { code: "Backspace", expected: "backspace" },
  { code: "Delete", expected: "delete" },
  { code: "Insert", expected: "insert" },
  { code: "Home", expected: "home" },
  { code: "End", expected: "end" },
  { code: "Pause", expected: "pause" },
  { code: "ArrowUp", expected: "up" },
  { code: "ArrowDown", expected: "down" },
  { code: "ArrowLeft", expected: "left" },
  { code: "ArrowRight", expected: "right" },
  { code: "KeyA", expected: "a" },
  { code: "Digit5", expected: "5" },
  { code: "F13", expected: "f13" },
  { code: "ShiftLeft", expected: "shift" },
  { code: "ControlLeft", expected: "ctrl" },
  { code: "AltLeft", expected: "alt", os: "linux" },
  { code: "AltLeft", expected: "option", os: "macos" },
  { code: "MetaLeft", expected: "super", os: "linux" },
  { code: "MetaLeft", expected: "command", os: "macos" },
];

for (const { code, expected, os = "linux" } of stableCases) {
  assert.equal(
    getKeyName(evt({ code }), os),
    expected,
    `${code} on ${os} must stay "${expected}"`,
  );
}

// Unknown CamelCase codes: no space insertion (AudioVolumeUp → audiovolumeup)
assert.equal(getKeyName(evt({ code: "AudioVolumeUp" })), "audiovolumeup");
assert.notEqual(getKeyName(evt({ code: "AudioVolumeUp" })), "audio volume up");

// ---------------------------------------------------------------------------
// normalizeKey: left/right modifiers + legacy spaced compound names
// ---------------------------------------------------------------------------
assert.equal(normalizeKey("left shift"), "shift");
assert.equal(normalizeKey("right ctrl"), "ctrl");
assert.equal(normalizeKey("scroll lock"), "scrolllock");
assert.equal(normalizeKey("page up"), "pageup");
assert.equal(normalizeKey("caps lock"), "capslock");
assert.equal(normalizeKey("scrolllock"), "scrolllock");
assert.equal(normalizeKey("space"), "space");

// ---------------------------------------------------------------------------
// formatKeyCombination: UI still shows friendly compound labels
// ---------------------------------------------------------------------------
assert.equal(formatKeyCombination("scrolllock", "linux"), "Scroll Lock");
assert.equal(formatKeyCombination("ctrl+pageup", "linux"), "Ctrl + Page Up");
assert.equal(formatKeyCombination("option+space", "macos"), "Option + Space");
assert.equal(
  formatKeyCombination("shift_left+f13", "linux"),
  "Left Shift + F13",
);

console.log("keyboard: all assertions passed");
