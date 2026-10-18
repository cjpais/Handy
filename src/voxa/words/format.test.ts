import assert from "node:assert/strict";
import {
  barRatio,
  formatDecimal,
  isoDay,
  normalizeRange,
  per100,
  periodFor,
  polylinePath,
  trendPoints,
  weekLabel,
} from "./format";

const today = new Date(2026, 9, 18); // 18 Oct 2026, local

// days
assert.equal(isoDay(today), "2026-10-18");
assert.equal(isoDay(new Date(2026, 0, 5)), "2026-01-05");

// presets are rolling windows ending today
assert.deepEqual(periodFor("week", today), {
  from: "2026-10-12",
  to: "2026-10-18",
});
assert.deepEqual(periodFor("month", today), {
  from: "2026-09-19",
  to: "2026-10-18",
});
assert.deepEqual(periodFor("all", today), { from: null, to: null });
// across a month and a year boundary
assert.deepEqual(periodFor("week", new Date(2026, 0, 3)), {
  from: "2025-12-28",
  to: "2026-01-03",
});
assert.deepEqual(
  periodFor("custom", today, { from: "2026-08-01", to: "2026-08-31" }),
  { from: "2026-08-01", to: "2026-08-31" },
);
assert.deepEqual(periodFor("custom", today, null), { from: null, to: null });

// custom ranges: reversed ends are swapped, incomplete ones refused
assert.deepEqual(normalizeRange("2026-08-31", "2026-08-01"), {
  from: "2026-08-01",
  to: "2026-08-31",
});
assert.equal(normalizeRange("", "2026-08-01"), null);
assert.equal(normalizeRange("2026-08", "2026-08-01"), null);

// rates
assert.equal(per100(5, 200), 2.5);
assert.equal(per100(0, 200), 0);
assert.equal(per100(3, 0), 0);
assert.equal(formatDecimal(4.236, "en"), "4.2");
assert.equal(formatDecimal(4.236, "ru"), "4,2");
assert.equal(formatDecimal(3, "en"), "3.0");

// week labels
assert.match(weekLabel("2026-10-12", "en"), /Oct/);
assert.match(weekLabel("2026-10-12", "en"), /12/);

// list bars: never invisible, never above 1
assert.equal(barRatio(0, 10), 0);
assert.equal(barRatio(10, 10), 1);
assert.equal(barRatio(5, 10), 0.5);
assert.equal(barRatio(1, 1000), 0.03);
assert.equal(barRatio(5, 0), 0);

// trend geometry
const flat = trendPoints([0, 0, 0], 100, 50);
assert.equal(flat.max, 0);
assert.ok(
  flat.points.every((p) => p.y === 44),
  "zero line on the bottom edge",
);
const rising = trendPoints([1, 2, 4], 100, 50);
assert.equal(rising.points.length, 3);
assert.ok(rising.points[0].y > rising.points[1].y);
assert.ok(rising.points[1].y > rising.points[2].y);
assert.equal(rising.points[0].x, 6);
assert.equal(rising.points[2].x, 94);
assert.ok(rising.points[2].y >= 6, "headroom keeps the peak inside the box");
const single = trendPoints([3], 100, 50);
assert.equal(single.points[0].x, 50);
assert.equal(polylinePath([]), "");
assert.equal(
  polylinePath([
    { x: 1, y: 2, value: 0 },
    { x: 3.04, y: 4, value: 0 },
  ]),
  "M1.0 2.0 L3.0 4.0",
);

console.log("words format: ok");
