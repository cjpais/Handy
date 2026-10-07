import assert from "node:assert/strict";
import {
  formatGamepadPart,
  formatGamepadCombination,
  sortGamepadButtons,
  STANDARD_GAMEPAD_BUTTON_MAP,
} from "./gamepad.ts";

// Test button formatting
assert.equal(formatGamepadPart("select"), "Select");
assert.equal(formatGamepadPart("back"), "Select");
assert.equal(formatGamepadPart("start"), "Start");
assert.equal(formatGamepadPart("menu"), "Start");
assert.equal(formatGamepadPart("lb"), "LB");
assert.equal(formatGamepadPart("rb"), "RB");
assert.equal(formatGamepadPart("lt"), "LT");
assert.equal(formatGamepadPart("rt"), "RT");
assert.equal(formatGamepadPart("a"), "A");
assert.equal(formatGamepadPart("b"), "B");
assert.equal(formatGamepadPart("x"), "X");
assert.equal(formatGamepadPart("y"), "Y");
assert.equal(formatGamepadPart("dpad_up"), "D-Pad Up");

// Test combination formatting
assert.equal(formatGamepadCombination("select + start"), "Select + Start");
assert.equal(formatGamepadCombination("select+start"), "Select + Start");
assert.equal(formatGamepadCombination("SELECT + START"), "Select + Start");
assert.equal(formatGamepadCombination(""), "Select + Start");
assert.equal(formatGamepadCombination("lb + rb"), "LB + RB");

// Test button sorting (canonical order: select before start)
const sorted = sortGamepadButtons(["start", "select"]);
assert.deepEqual(sorted, ["select", "start"]);

// Test standard button mapping
assert.equal(STANDARD_GAMEPAD_BUTTON_MAP[8], "select");
assert.equal(STANDARD_GAMEPAD_BUTTON_MAP[9], "start");

console.log("gamepad: all assertions passed");
