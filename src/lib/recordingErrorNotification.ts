import type { RecordingErrorEvent } from "./types/events";

export interface RecordingErrorNotification {
  level: "error" | "warning";
  titleKey: string;
  titleValues?: Record<string, string>;
  descriptionKey?: string;
  descriptionFallbackKey?: string;
}

export const getRecordingErrorNotification = (
  event: RecordingErrorEvent,
  currentPlatform: string,
): RecordingErrorNotification => {
  if (event.error_type === "silent_input") {
    return {
      level: "warning",
      titleKey: "errors.silentInputTitle",
      descriptionKey: "errors.silentInput",
    };
  }

  if (event.error_type === "microphone_permission_denied") {
    return {
      level: "error",
      titleKey: "errors.micPermissionDeniedTitle",
      descriptionKey: `errors.micPermissionDenied.${currentPlatform}`,
      descriptionFallbackKey: "errors.micPermissionDenied.generic",
    };
  }

  if (event.error_type === "no_input_device") {
    return {
      level: "error",
      titleKey: "errors.noInputDeviceTitle",
      descriptionKey: "errors.noInputDevice",
    };
  }

  return {
    level: "error",
    titleKey: "errors.recordingFailed",
    titleValues: { error: event.detail ?? "Unknown error" },
  };
};
