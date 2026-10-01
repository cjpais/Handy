import assert from "node:assert/strict";
import { copyToClipboard } from "./clipboard";

const successfulClipboard = {
  writeText: async () => {},
};

const failedClipboard = {
  writeText: async () => {
    throw new Error("clipboard unavailable");
  },
};

assert.equal(await copyToClipboard("copied text", successfulClipboard), true);
assert.equal(await copyToClipboard("copied text", failedClipboard), false);

console.log("clipboard: all assertions passed");
