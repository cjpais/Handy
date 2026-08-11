export const PERMISSION_POLL_TIMEOUT_MS = 15_000;
export const MAX_PERMISSION_POLLING_ERRORS = 3;

export type PermissionStatus = "checking" | "needed" | "waiting" | "granted";

export interface PermissionPair<T> {
  accessibility: T;
  microphone: T;
}

export interface PermissionPollOutcome {
  permissions: PermissionPair<PermissionStatus>;
  timedOut: PermissionPair<boolean>;
  allGranted: boolean;
}

export type PermissionPollingErrorEvent = "start" | "success" | "failure";

export interface PermissionPollingErrorOutcome {
  consecutiveErrors: number;
  shouldStop: boolean;
}

export function permissionPollingErrorOutcome(
  currentErrorCount: number,
  event: PermissionPollingErrorEvent,
): PermissionPollingErrorOutcome {
  const consecutiveErrors = event === "failure" ? currentErrorCount + 1 : 0;

  return {
    consecutiveErrors,
    shouldStop:
      event === "failure" && consecutiveErrors >= MAX_PERMISSION_POLLING_ERRORS,
  };
}

export function permissionStatusAfterPoll(
  current: PermissionStatus,
  granted: boolean,
  waitingSince: number | null,
  now: number,
): PermissionStatus {
  if (granted) return "granted";

  if (
    current === "waiting" &&
    waitingSince !== null &&
    now - waitingSince >= PERMISSION_POLL_TIMEOUT_MS
  ) {
    return "needed";
  }

  return current;
}

export function permissionPollOutcome(
  current: PermissionPair<PermissionStatus>,
  granted: PermissionPair<boolean>,
  waitingSince: PermissionPair<number | null>,
  now: number,
): PermissionPollOutcome {
  const permissions = {
    accessibility: permissionStatusAfterPoll(
      current.accessibility,
      granted.accessibility,
      waitingSince.accessibility,
      now,
    ),
    microphone: permissionStatusAfterPoll(
      current.microphone,
      granted.microphone,
      waitingSince.microphone,
      now,
    ),
  };

  const timedOut = {
    accessibility:
      current.accessibility === "waiting" &&
      !granted.accessibility &&
      waitingSince.accessibility !== null &&
      now - waitingSince.accessibility >= PERMISSION_POLL_TIMEOUT_MS,
    microphone:
      current.microphone === "waiting" &&
      !granted.microphone &&
      waitingSince.microphone !== null &&
      now - waitingSince.microphone >= PERMISSION_POLL_TIMEOUT_MS,
  };

  return {
    permissions,
    timedOut,
    allGranted:
      permissions.accessibility === "granted" &&
      permissions.microphone === "granted",
  };
}
