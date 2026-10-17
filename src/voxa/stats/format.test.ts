import assert from "node:assert/strict";
import {
  buildBars,
  formatPercent,
  modelLabel,
  shortDate,
  splitModelId,
  splitDuration,
  splitMinutes,
} from "./format";

// percent
assert.equal(formatPercent(0.4231), "42%");
assert.equal(formatPercent(1), "100%");
assert.equal(formatPercent(0), "0%");
assert.equal(formatPercent(null), "—");
assert.equal(formatPercent(undefined), "—");

// durations
assert.deepEqual(splitDuration(850), { value: 850, unit: "ms" });
assert.deepEqual(splitDuration(999.4), { value: 999, unit: "ms" });
assert.deepEqual(splitDuration(1250), { value: 1.3, unit: "s" });
assert.deepEqual(splitDuration(12_000), { value: 12, unit: "s" });
assert.deepEqual(splitMinutes(0.5), { value: 0.5, unit: "min" });
assert.deepEqual(splitMinutes(119.96), { value: 120, unit: "min" });
assert.deepEqual(splitMinutes(150), { value: 2.5, unit: "h" });

// bars
const day = (date: string, words: number, dictations = 1) => ({
  date,
  words,
  dictations,
});
const { bars, max } = buildBars([
  day("2026-03-09", 100),
  day("2026-03-10", 0, 0),
  day("2026-03-11", 1),
]);
assert.equal(max, 100);
assert.equal(bars[0].ratio, 1);
assert.equal(bars[1].ratio, 0, "an empty day has no bar");
assert.equal(bars[2].ratio, 0.04, "a tiny day is still visible");
assert.equal(bars[2].isToday, true);
assert.equal(bars[0].isToday, false);
assert.deepEqual(buildBars([day("2026-03-11", 0, 0)]).bars[0].ratio, 0);
assert.equal(buildBars([]).max, 0);

// labels
assert.equal(shortDate("2026-03-09"), "09.03");
assert.equal(
  modelLabel(
    "handy-computer/whisper-large-v3-turbo-gguf/whisper-large-v3-turbo-Q8_0.gguf",
  ),
  "whisper-large-v3-turbo-Q8_0",
);
assert.equal(modelLabel("small"), "small");
assert.deepEqual(
  splitModelId(
    "handy-computer/whisper-large-v3-turbo-gguf/whisper-large-v3-turbo-Q8_0.gguf",
  ),
  { name: "whisper-large-v3-turbo", quant: "Q8_0" },
);
assert.deepEqual(splitModelId("x/y/parakeet-0.6b-Q4_K_M.gguf"), {
  name: "parakeet-0.6b",
  quant: "Q4_K_M",
});
assert.deepEqual(splitModelId("x/y/model-F16.gguf"), {
  name: "model",
  quant: "F16",
});
assert.deepEqual(splitModelId("small"), { name: "small", quant: null });

console.log("voxa stats format: all assertions passed");
