import assert from "node:assert/strict";
import { matchesSearch, splitByQuery } from "./search";

assert.equal(matchesSearch("Buy milk tomorrow", "MILK"), true);
assert.equal(matchesSearch("Встреча в ПЯТНИЦУ", "пятницу"), true);
assert.equal(matchesSearch("Buy milk tomorrow", "bread"), false);
assert.equal(matchesSearch("anything", "  "), true);

assert.deepEqual(splitByQuery("Buy milk, more Milk", "milk"), [
  { text: "Buy ", match: false },
  { text: "milk", match: true },
  { text: ", more ", match: false },
  { text: "Milk", match: true },
]);

// Non-ASCII case folding keeps the original casing of the matched text
assert.deepEqual(splitByQuery("Встреча в ПЯТНИЦУ", "пятницу"), [
  { text: "Встреча в ", match: false },
  { text: "ПЯТНИЦУ", match: true },
]);

// Regex syntax in the query is matched literally
assert.deepEqual(splitByQuery("costs $5 (approx.)", "(approx.)"), [
  { text: "costs $5 ", match: false },
  { text: "(approx.)", match: true },
]);

assert.deepEqual(splitByQuery("no hits here", "zzz"), [
  { text: "no hits here", match: false },
]);
assert.deepEqual(splitByQuery("unchanged", " "), [
  { text: "unchanged", match: false },
]);

console.log("search: all assertions passed");
