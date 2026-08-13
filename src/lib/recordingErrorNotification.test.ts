// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/recordingErrorNotification.test.ts
import assert from "node:assert";
import { getRecordingErrorNotification } from "./recordingErrorNotification";

assert.deepEqual(
  getRecordingErrorNotification({ error_type: "silent_input" }, "linux"),
  {
    level: "warning",
    titleKey: "errors.silentInputTitle",
    descriptionKey: "errors.silentInput",
  },
);

assert.equal(
  getRecordingErrorNotification(
    { error_type: "microphone_permission_denied" },
    "macos",
  ).descriptionKey,
  "errors.micPermissionDenied.macos",
);

assert.equal(
  getRecordingErrorNotification({ error_type: "no_input_device" }, "linux")
    .level,
  "error",
);

assert.deepEqual(
  getRecordingErrorNotification(
    { error_type: "unexpected", detail: "capture failed" },
    "linux",
  ),
  {
    level: "error",
    titleKey: "errors.recordingFailed",
    titleValues: { error: "capture failed" },
  },
);

console.log("recordingErrorNotification: all assertions passed");
