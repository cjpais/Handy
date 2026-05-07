# Curano LiveSTT Integration Plan

## Purpose

Curano AI Dictate is a Curano AI fork of Handy. It preserves Handy's local transcription backend and adds Curano LiveSTT server-based transcription.

The Local backend remains available for offline, on-device transcription. The Curano LiveSTT backend sends microphone audio to a configured Curano LiveSTT server over WebSocket and receives live transcription events from that server.

## Product naming

- Formal name: Curano AI Dictate
- Short name: Curano Dictate
- Repository name: Curano-AI-Dictate
- Recommended future bundle identifier: `ai.curano.dictate`
- Forked from Handy by cjpais
- Local backend remains available

## Existing Handy Behavior

The existing local flow is local-first:

1. A configurable shortcut starts recording.
2. The same shortcut, or push-to-talk release, stops recording.
3. Recorded audio is processed locally.
4. A local STT model such as Whisper or Parakeet returns final text.
5. The app runs the existing post-processing, history, and paste flow.
6. The final text is pasted into the currently focused application.

This behavior must remain available when the Local backend is selected.

## Implemented LiveSTT behavior

The Curano LiveSTT flow adds a server-based backend without replacing local transcription:

1. A configurable shortcut starts a LiveSTT transcription session.
2. The app authenticates and connects to the LiveSTT WebSocket endpoint.
3. The app streams binary PCM audio chunks while recording.
4. The app receives `partial` and `final` transcription events.
5. Shortcut release or stop ends local microphone capture.
6. The app sends `stop_record` over the existing WebSocket.
7. The app keeps the WebSocket open and waits for `session_ended`.
8. The app uses the accumulated final text.
9. The app runs the existing post-processing, history, and paste flow.
10. The final text is pasted once into the active application.
11. The app closes the WebSocket transport as cleanup.

Partial text is for overlay or debug display only. It must not be pasted into the active application.

LiveSTT auth stores access and refresh tokens in memory only. Login returns both tokens. Before starting a session, the app refreshes the access token if it is near expiry by calling `POST /auth/refresh` with `{ "refresh_token": "..." }`. If WebSocket auth is rejected during handshake, recording, or finalization, the app attempts up to two bounded refresh/reconnect attempts per LiveSTT session and replays all PCM chunks captured for the current recording. If replay itself fails after reconnect, the app aborts the session safely and surfaces the error. The refresh response replaces both tokens. Unauthorized or forbidden refresh responses clear tokens and require login again.

## Protocol

### Endpoint

LiveSTT sessions use the configured HTTP or HTTPS server base URL and connect to:

```text
ws(s)://SERVER/api/ws/live-transcription?token=ACCESS_TOKEN&audio_format=pcm
```

The server URL is configurable rather than hard-coded to production infrastructure.

Production deployments should use HTTPS so the desktop client connects over `wss`. Local mock and localhost testing may use plain HTTP.

### Query parameters

- `token`: required JWT access token.
- `audio_format`: optional. Supported API values are `webm_opus` and `pcm`. The API default is `webm_opus`; the desktop MVP target is `pcm`.
- `consultation_id`: optional. Links the transcription session to a consultation.

### Events

`session_started` starts the server session. The desktop client does not require
this event before sending the first PCM audio frame, because production servers
may emit it only after audio has arrived:

```json
{ "type": "session_started", "session_id": 123 }
```

`partial` provides intermediate text that may change:

```json
{
  "type": "partial",
  "session_id": 123,
  "text": "Привет как дел...",
  "is_final": false
}
```

`final` provides stable text:

```json
{
  "type": "final",
  "session_id": 123,
  "text": "Привет, как дела?",
  "is_final": true,
  "start_time": 0.0,
  "end_time": 2.5
}
```

`error` reports protocol or server errors:

```json
{
  "type": "error",
  "session_id": 123,
  "error_code": "CONNECTION_CLOSED",
  "error_message": "..."
}
```

`session_ended` marks protocol-level completion:

```json
{ "type": "session_ended", "session_id": 123 }
```

### Text accumulation semantics

Curano LiveSTT desktop accumulates `final.text` chunks in order and pastes the accumulated final transcript. It also tolerates cumulative final text by replacing the accumulated value when a new final starts with the existing accumulated final.

The app should track:

- `finalizedText`: accumulated stable final transcript.
- `pendingPartial`: latest temporary partial text.
- `currentText`: accumulated final plus pending partial for overlay/debug preview.

Partial text is only for temporary display and must not overwrite accumulated final text. The paste flow must use accumulated final text.

### Stop protocol

Normal completion is not done by closing the WebSocket.

Correct normal stop flow:

1. User releases the shortcut or otherwise stops dictation.
2. Desktop app stops local microphone capture.
3. Desktop app stops sending binary audio chunks.
4. Desktop app sends the protocol command `stop_record` over the existing WebSocket.
5. Desktop app keeps the WebSocket open.
6. Server may send remaining `final` events.
7. Server sends `session_ended`.
8. Desktop app uses the accumulated final text.
9. App runs existing post-processing, history, and paste behavior.
10. App closes the WebSocket transport as cleanup.

Closing the WebSocket immediately after stopping microphone capture is incorrect because the server may not have finalized the last audio.

Transport `close()` is for manual disconnect, cancellation, cleanup after `session_ended`, and error handling. It is not the protocol-level finish-transcription signal.

### Error handling

On protocol or transport error, the app should close the transport and avoid pasting unless a future implementation defines an explicitly safe fallback. Error details should be surfaced without logging secrets.

If finalization times out, the app should use accumulated final text if available. Partial text is only for preview/debug display and should not be pasted as fallback.

### Cancellation behavior

Cancel should close the transport, discard current LiveSTT text, and skip paste. Cancellation is distinct from normal stop.

## Audio format

The desktop MVP should use raw PCM:

- Mono
- 16000 Hz
- Signed i16
- Little-endian
- No WAV header
- Binary WebSocket frames
- Chunks around 250 ms

The desktop app streams raw resampled 16 kHz mono PCM frames to LiveSTT before local VAD filtering. Local transcription continues to use the existing VAD-filtered path.

At 16 kHz, a 250 ms PCM chunk is approximately:

```text
16000 samples/sec * 0.25 sec = 4000 samples
4000 samples * 2 bytes = 8000 bytes
```

The browser prototype can use `MediaRecorder` and WebM/Opus easily. The Tauri/Rust desktop app already captures and resamples audio internally, so sending raw 16 kHz PCM chunks is simpler and less error-prone than producing WebM/Opus from Rust for the MVP.

## Architecture plan

### Backend selection

Curano AI Dictate should support:

- Local backend: existing Handy behavior. Records audio locally, runs local STT on stop, then post-processes and pastes final text. This mode can remain offline.
- Curano LiveSTT backend: requires authentication, streams microphone audio to LiveSTT while recording, waits for server finalization on stop, then pastes accumulated final text.

### Implemented Rust modules

- `src-tauri/src/livestt/mod.rs`
- `src-tauri/src/livestt/auth.rs`
- `src-tauri/src/livestt/client.rs`
- `src-tauri/src/livestt/events.rs`
- `src-tauri/src/livestt/audio.rs`
- `src-tauri/src/livestt/session.rs`

### Implemented settings

- `transcription_backend`: `local` or `live_stt`
- `livestt_server_url`
- `livestt_audio_format`: MVP value `pcm`
- `livestt_consultation_id`
- `livestt_finalize_timeout_ms`

### Implemented frontend

- Backend selector with Local model and Curano LiveSTT options.
- LiveSTT settings section with server URL, login/logout, auth status, consultation ID, finalize timeout, and privacy notice.

### Implemented security

- JWT access tokens are kept in memory and are not stored in normal settings.
- Token values, passwords, and credentials must not be logged.
- Password inputs are cleared after login attempts.
- A later production version may use OS keychain storage or a refresh-token flow.

## Implementation phases

LiveSTT was implemented in small, reviewable phases. Completed areas:

1. Settings and backend selector.
2. Authentication.
3. WebSocket client.
4. PCM conversion.
5. Recorder streaming hook for raw resampled PCM before local VAD.
6. LiveSTT session manager.
7. Action integration.
8. Frontend settings.
9. Mock server and testing.
10. History metadata.
11. Documentation and privacy polish.

Known limitations after this implementation:

- LiveSTT access and refresh tokens are memory-only and are not persisted across app restarts.
- Desktop LiveSTT uses PCM. WebM/Opus desktop encoding is not implemented.
- LiveSTT requires a reachable configured server and successful login before recording.
- Curano LiveSTT is the current/default backend.
- Local model transcription remains available in Settings for on-device transcription.

## Non-goals for first implementation

- No live typing into the active application.
- No removal of local STT.
- No token persistence in plain settings.
- No WebM/Opus desktop encoding in the MVP.
- No upstream PR to Handy yet.

## Acceptance criteria

The LiveSTT implementation should satisfy these conditions:

- Local backend still works.
- LiveSTT streams PCM chunks.
- Normal stop sends `stop_record`.
- Normal stop waits for `session_ended`.
- Final text is pasted once.
- Partial text is never pasted.
- JWT access token is not persisted in settings.
