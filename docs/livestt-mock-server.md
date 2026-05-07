# LiveSTT Mock Server

This repository includes a local Curano LiveSTT WebSocket mock for manual and integration testing. It is a test utility only; it does not use real credentials and does not contact Curano services.

## Start The Server

```bash
bun run livestt:mock
```

By default it listens on:

```text
http://127.0.0.1:8787
```

Optional host and port overrides:

```bash
LIVESTT_MOCK_HOST=127.0.0.1 LIVESTT_MOCK_PORT=8790 bun run livestt:mock
```

The script itself is dependency-free Node.js and can also be run directly:

```bash
node scripts/livestt_mock_server.mjs
```

The app setting should use the HTTP base URL, not the WebSocket path:

```text
http://127.0.0.1:8787
```

Use the mock login tokens or any other non-empty test tokens. The examples below use:

```text
mock-access-token
mock-refresh-token
```

The mock also supports the app's local login flow:

```text
POST /auth/login
```

It returns `{"access_token":"mock-access-token","refresh_token":"mock-refresh-token"}` for any submitted credentials and logs only whether a username was present.

The mock also supports refresh:

```text
POST /auth/refresh
```

It accepts JSON with a non-empty `refresh_token` and returns `{"access_token":"mock-access-token-refreshed","refresh_token":"mock-refresh-token-refreshed"}`. It logs only `refresh_token_present=true/false`, never the token or raw request body. These tokens are mock-only; do not use real credentials with the mock server.

Do not use real credentials with the mock server.

## Protocol Behavior

The mock accepts WebSocket connections only at:

```text
/api/ws/live-transcription
```

It validates these query parameters:

- `token` must exist.
- `audio_format` must be `pcm`.
- `consultation_id` is optional.

After connection it sends:

```json
{ "type": "session_started", "session_id": 123 }
```

It counts binary audio frames and bytes without decoding the audio. When it receives this text command:

```json
{ "type": "stop_record" }
```

it sends:

```json
{
  "type": "final",
  "session_id": 123,
  "text": "mock final text",
  "is_final": true,
  "start_time": 0.0,
  "end_time": 1.0
}
```

then:

```json
{ "type": "session_ended", "session_id": 123 }
```

Curano LiveSTT desktop accumulates `final.text` chunks in order and pastes the accumulated final transcript. `partial.text` is kept separately for overlay/debug preview and must not overwrite accumulated final text. The mock emits a single final chunk by default.

The expected production client behavior is to close the WebSocket transport after `session_ended`.

## Expected Logs

The mock logs:

- connection requests with sanitized query params;
- connection opened;
- binary frame count and total bytes;
- `stop_record` received;
- `final` and `session_ended` sent;
- client close status.

Token values are not logged. The log only records whether the `token` parameter was present.

To simulate auth expiration after the first binary frame on the first WebSocket connection:

```bash
LIVESTT_MOCK_EXPIRE_FIRST_WS_AFTER_FIRST_BINARY=1 bun run livestt:mock
```

The mock closes that socket with code `4001` and accepts the refreshed/replayed connection.

To simulate auth rejection during the first WebSocket handshake:

```bash
LIVESTT_MOCK_REJECT_FIRST_WS_HANDSHAKE=401 bun run livestt:mock
LIVESTT_MOCK_REJECT_FIRST_WS_HANDSHAKE=403 bun run livestt:mock
```

The mock returns HTTP `401` or `403` for the first upgrade only. The refreshed retry succeeds.

To emit multiple incremental final chunks after `stop_record`:

```bash
LIVESTT_MOCK_FINAL_CHUNKS=1 bun run livestt:mock
```

The mock sends `final.text` values `mock` and `final text`, then `session_ended`. The desktop client should paste `mock final text`.

## Manual Test Checklist

1. Start the mock server with `bun run livestt:mock`.
2. Open Curano AI Dictate settings.
3. Select the Curano LiveSTT backend.
4. Set the LiveSTT server URL to `http://127.0.0.1:8787`.
5. Log in with any local test username/password, or inject `mock-access-token` if you are testing below the UI.
6. Record a short phrase.
7. Stop recording normally.
8. Verify the mock logs at least one binary frame.
9. Verify the mock logs `stop_record received`.
10. Verify the app receives `mock final text` and pastes it once.
11. Verify the mock logs `final and session_ended sent; waiting for client close`.
12. Verify the mock logs `client closed status=after_session_ended`.

If the client closes before `stop_record`, normal LiveSTT completion is not following the protocol.
