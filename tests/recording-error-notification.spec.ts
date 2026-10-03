import assert from "node:assert";
import { test } from "@playwright/test";
import { getRecordingErrorNotification } from "../src/lib/recordingErrorNotification";

test("recording error mapping distinguishes input silence and capture failures", () => {
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
});
