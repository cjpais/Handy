import assert from "node:assert";
import {
  PERMISSION_POLL_TIMEOUT_MS,
  permissionPollOutcome,
  permissionPollingErrorOutcome,
  permissionStatusAfterPoll,
} from "./permissionPolling";

assert.equal(
  permissionStatusAfterPoll(
    "waiting",
    false,
    1_000,
    1_000 + PERMISSION_POLL_TIMEOUT_MS - 1,
  ),
  "waiting",
  "a pending request should keep waiting before the timeout",
);

assert.equal(
  permissionStatusAfterPoll(
    "waiting",
    false,
    1_000,
    1_000 + PERMISSION_POLL_TIMEOUT_MS,
  ),
  "needed",
  "an undetected permission should become retryable at the timeout",
);

assert.equal(
  permissionStatusAfterPoll("waiting", true, 1_000, 1_001),
  "granted",
  "a granted permission should complete immediately",
);

assert.equal(
  permissionStatusAfterPoll("needed", false, null, 99_000),
  "needed",
  "a permission that was not requested should remain idle",
);

assert.equal(
  permissionStatusAfterPoll("granted", false, null, 99_000),
  "granted",
  "a transient false poll must not revoke a grant already observed by this screen",
);

const preservedGrantCompletion = permissionPollOutcome(
  { accessibility: "granted", microphone: "waiting" },
  { accessibility: false, microphone: true },
  { accessibility: null, microphone: 1_000 },
  2_000,
);

assert.deepEqual(
  preservedGrantCompletion.permissions,
  { accessibility: "granted", microphone: "granted" },
  "the combined outcome should preserve a previously observed grant",
);
assert.equal(
  preservedGrantCompletion.allGranted,
  true,
  "completion must use the resulting permission state rather than raw poll booleans",
);

const firstStaggeredPoll = permissionPollOutcome(
  { accessibility: "waiting", microphone: "waiting" },
  { accessibility: true, microphone: false },
  { accessibility: 1_000, microphone: 1_000 },
  2_000,
);
assert.deepEqual(firstStaggeredPoll.permissions, {
  accessibility: "granted",
  microphone: "waiting",
});
assert.equal(firstStaggeredPoll.allGranted, false);

const onePermissionTimeout = permissionPollOutcome(
  firstStaggeredPoll.permissions,
  { accessibility: false, microphone: false },
  { accessibility: null, microphone: 1_000 },
  1_000 + PERMISSION_POLL_TIMEOUT_MS,
);
assert.deepEqual(onePermissionTimeout.timedOut, {
  accessibility: false,
  microphone: true,
});
assert.deepEqual(onePermissionTimeout.permissions, {
  accessibility: "granted",
  microphone: "needed",
});

const simultaneousGrant = permissionPollOutcome(
  { accessibility: "waiting", microphone: "waiting" },
  { accessibility: true, microphone: true },
  { accessibility: 1_000, microphone: 1_000 },
  1_001,
);
assert.equal(simultaneousGrant.allGranted, true);

let errorState = permissionPollingErrorOutcome(0, "failure");
assert.deepEqual(errorState, { consecutiveErrors: 1, shouldStop: false });
errorState = permissionPollingErrorOutcome(
  errorState.consecutiveErrors,
  "failure",
);
errorState = permissionPollingErrorOutcome(
  errorState.consecutiveErrors,
  "failure",
);
assert.deepEqual(errorState, { consecutiveErrors: 3, shouldStop: true });

errorState = permissionPollingErrorOutcome(
  errorState.consecutiveErrors,
  "start",
);
assert.deepEqual(
  errorState,
  { consecutiveErrors: 0, shouldStop: false },
  "a retry must start with a fresh consecutive-error budget",
);
errorState = permissionPollingErrorOutcome(
  errorState.consecutiveErrors,
  "failure",
);
assert.deepEqual(
  errorState,
  { consecutiveErrors: 1, shouldStop: false },
  "the first failure after retry must not stop polling",
);

console.log("permissionPolling: all assertions passed");
