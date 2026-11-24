import assert from "node:assert/strict";
import { formatKeyCombination, getKeyName, recordChordKeys } from "./keyboard";

const keyboardEvent = (value: {
  code?: string;
  key?: string;
  metaKey?: boolean;
}): KeyboardEvent => value as KeyboardEvent;

const compoundKeys = [
  ["ScrollLock", "scrolllock", "Scroll Lock"],
  ["CapsLock", "capslock", "Caps Lock"],
  ["NumLock", "numlock", "Num Lock"],
  ["PageUp", "pageup", "Page Up"],
  ["PageDown", "pagedown", "Page Down"],
  ["PrintScreen", "printscreen", "Print Screen"],
] as const;

for (const [code, stored, displayed] of compoundKeys) {
  assert.equal(getKeyName(keyboardEvent({ code })), stored);
  assert.equal(formatKeyCombination(stored, "linux"), displayed);
}

assert.equal(getKeyName(keyboardEvent({ key: "CapsLock" })), "capslock");
assert.equal(
  getKeyName(keyboardEvent({ code: "AudioVolumeUp" })),
  "audiovolumeup",
);

// Linux window managers may swallow Super keydown while setting metaKey on the companion key.
assert.equal(
  recordChordKeys(
    [],
    keyboardEvent({ code: "KeyZ", metaKey: true }),
    "linux",
  ).join("+"),
  "super+z",
);

const afterExplicitSuper = recordChordKeys(
  [],
  keyboardEvent({ code: "MetaLeft", metaKey: true }),
  "linux",
);
assert.equal(
  recordChordKeys(
    afterExplicitSuper,
    keyboardEvent({ code: "KeyZ", metaKey: true }),
    "linux",
  ).join("+"),
  "super+z",
);

assert.equal(
  recordChordKeys([], keyboardEvent({ code: "KeyZ" }), "linux").join("+"),
  "z",
);

console.log("keyboard: all assertions passed");
