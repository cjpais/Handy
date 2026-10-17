import assert from "node:assert/strict";
import {
  BAR_COUNT,
  BAR_MAX_PX,
  BAR_MIN_PX,
  barHeightPx,
  barLevels,
  formatElapsed,
  rms,
  smoothLevels,
} from "./levels";

const sixteen = (value: number) => Array<number>(16).fill(value);

// timer
assert.equal(formatElapsed(0), "0:00");
assert.equal(formatElapsed(7), "0:07");
assert.equal(formatElapsed(75), "1:15");
assert.equal(formatElapsed(3600), "60:00");
assert.equal(formatElapsed(-4), "0:00");
assert.equal(formatElapsed(7.9), "0:07");

// rms
assert.equal(rms([]), 0);
assert.equal(rms(sixteen(0)), 0);
assert.ok(Math.abs(rms(sixteen(0.5)) - 0.5) < 1e-12);
assert.ok(Math.abs(rms([1, 0]) - Math.sqrt(0.5)) < 1e-12);
assert.equal(rms([Number.NaN, 1]), Math.sqrt(0.5), "NaN counts as silence");

// smoothing moves a fraction of the way and converges
const step1 = smoothLevels(sixteen(0), sixteen(1), 0.3);
assert.ok(Math.abs(step1[0] - 0.3) < 1e-12);
let level = sixteen(0);
for (let i = 0; i < 40; i += 1) level = smoothLevels(level, sixteen(1), 0.3);
assert.ok(level[0] > 0.999);
assert.ok(
  smoothLevels(sixteen(0.4), sixteen(0.4)).every(
    (v) => Math.abs(v - 0.4) < 1e-12,
  ),
  "a steady input stays put",
);
assert.equal(smoothLevels([], [1, 1]).length, 2, "grows to the longer input");
assert.ok(
  smoothLevels(sixteen(0), sixteen(7), 1).every((v) => v === 1),
  "out-of-range input is clamped",
);

// bars
const silent = barLevels(sixteen(0));
assert.equal(silent.length, BAR_COUNT);
assert.ok(silent.every((v) => v === 0));
assert.ok(silent.map((v) => barHeightPx(v)).every((h) => h === BAR_MIN_PX));

const loud = barLevels(sixteen(1));
assert.ok(
  loud.every((v) => v <= 1 && v > 0.55),
  "loud input lifts every bar",
);
assert.equal(barHeightPx(1), BAR_MAX_PX);
assert.equal(barHeightPx(0), BAR_MIN_PX);
assert.equal(barHeightPx(2), BAR_MAX_PX, "clamped above");
assert.equal(barHeightPx(-1), BAR_MIN_PX, "clamped below");

// uniform input gives a symmetric envelope with the centre on top
const mid = barLevels(sixteen(0.5));
for (let i = 0; i < BAR_COUNT; i += 1) {
  assert.ok(
    Math.abs(mid[i] - mid[BAR_COUNT - 1 - i]) < 1e-9,
    `bar ${i} mirrors`,
  );
}
assert.ok(mid[3] > mid[0], "centre bar is tallest");

// louder input never makes a bar shorter
let previous = barLevels(sixteen(0));
for (let v = 0.1; v <= 1.0001; v += 0.1) {
  const next = barLevels(sixteen(v));
  next.forEach((value, i) =>
    assert.ok(value >= previous[i], `bar ${i} monotonic at ${v}`),
  );
  previous = next;
}

// the envelope is mirrored for any input, and lows drive the centre
const spike = sixteen(0);
spike[0] = 1;
const spiked = barLevels(spike);
for (let i = 0; i < BAR_COUNT; i += 1) {
  assert.ok(
    Math.abs(spiked[i] - spiked[BAR_COUNT - 1 - i]) < 1e-12,
    `spike mirrors at ${i}`,
  );
}
assert.ok(
  spiked[3] > spiked[0],
  "a loud low bucket lifts the centre above the edges",
);
const highs = sixteen(0);
highs[15] = 1;
assert.ok(
  barLevels(highs)[0] > barLevels(highs)[3],
  "a loud high bucket lifts the outer bars most",
);

console.log("voxa overlay levels: all assertions passed");
