# Handy local IPC API

Handy hosts a local IPC server so other applications on the same machine can
use it as the machine's transcription service — no bundled models, no separate
inference stack. The server speaks JSON-RPC 2.0 over a per-user local socket
and is enabled by default. Set `HANDY_NO_IPC=1` before launching Handy to
disable it.

**Protocol version: 1.** Additive changes (new methods, new response fields)
do not bump the version; breaking changes do. Clients should ignore response
fields they don't recognize.

## Trust model

The socket is same-user only (Unix socket file with `0600` permissions; named
pipe with the default same-user descriptor on Windows). This is the same trust
tier as Handy's existing remote controls (`SIGUSR2`,
`--toggle-transcription`): any process running as you can use it. Nothing
privacy-sensitive is exposed — no history, no dictated-text events, no
microphone access. Transcription runs only on audio the caller already has.

## Endpoint

| Platform | Endpoint                                                    |
| -------- | ----------------------------------------------------------- |
| macOS    | `~/Library/Application Support/com.pais.handy/handy.sock`   |
| Linux    | `$XDG_DATA_HOME/com.pais.handy/handy.sock` (usually `~/.local/share/...`) |
| Windows  | `\\.\pipe\com.pais.handy.ipc`                               |

In portable mode (Windows), the socket lives in the portable `Data` directory
instead. The socket exists only while Handy is running; a leftover socket file
after a crash is removed by the next launch. Clients that get "connection
refused" on an existing socket file should treat Handy as not running.

## Framing

Newline-delimited JSON: one JSON-RPC 2.0 message per line, UTF-8, `\n`
terminated. Requests are capped at 256 KB per line. Requests without an `id`
are notifications and receive no response. Requests on one connection are
handled sequentially; open multiple connections for concurrent calls (actual
inference is still serialized — see Concurrency).

```
→ {"jsonrpc":"2.0","id":1,"method":"status"}
← {"jsonrpc":"2.0","id":1,"result":{...}}
```

## Methods

### `initialize`

Optional version handshake. Call it first if you care about protocol
compatibility; servers reject explicitly-requested versions they don't speak
with `protocol-mismatch`.

```
→ {"jsonrpc":"2.0","id":1,"method":"initialize",
   "params":{"protocol_version":1,"client":"pi-transcribe/0.1"}}
← {"jsonrpc":"2.0","id":1,"result":{
     "server":"handy","protocol_version":1,"app_version":"0.9.5"}}
```

### `status`

Cheap availability snapshot. Always answers, even mid-dictation or with no
model installed.

```
→ {"jsonrpc":"2.0","id":2,"method":"status"}
← {"jsonrpc":"2.0","id":2,"result":{
     "server":"handy",
     "protocol_version":1,
     "app_version":"0.9.5",
     "selected_model":"whisper-large-v3-turbo",
     "selected_model_installed":true,
     "model_loaded":true,
     "loaded_model":"whisper-large-v3-turbo",
     "busy":false}}
```

`busy` is true while any job holds the engine: a dictation session (from
recording start through transcription), a history retranscription, an IPC
call, or maintenance (model switch/unload).
`model_loaded: false` is not an error condition — the first `transcribe.file`
call will load the model and report the cost in `load_ms`.

### `models.list`

The model registry (catalog + on-disk + custom), same shape as
`handy --list-models --json`. Each entry includes `id`, `name`,
`is_downloaded`, `engine_type`, `supported_languages`, and scoring metadata.

```
→ {"jsonrpc":"2.0","id":3,"method":"models.list"}
← {"jsonrpc":"2.0","id":3,"result":{"models":[...]}}
```

### `transcribe.file`

Batch-transcribe a WAV file with the user's selected model. The path must be
absolute; the file must be readable by the user Handy runs as.

```
→ {"jsonrpc":"2.0","id":4,"method":"transcribe.file",
   "params":{"path":"/tmp/recording.wav"}}
← {"jsonrpc":"2.0","id":4,"result":{
     "text":"hello world",
     "model":"whisper-large-v3-turbo",
     "audio_secs":3.2,
     "transcribe_ms":410,
     "load_ms":0}}
```

**Audio contract.** PCM WAV only (no mp3/m4a/ogg — convert first, e.g.
`ffmpeg -i in.m4a out.wav`). Accepted: 16/24/32-bit integer or 32-bit float
samples, any sample rate from 8 kHz to 192 kHz, 1–8 channels, up to 2 hours.
Handy normalizes to the engine's 16 kHz mono internally (channel averaging +
resampling), so callers do not need to resample.

There is no `model` parameter: v1 always uses the model selected in the app.
There is no request cancellation in v1; disconnecting does not abort a running
transcription.

## Errors

Errors follow JSON-RPC (`error.code`, `error.message`) plus a stable
machine-readable `error.data.kind`. Match on `kind` (or `code`), never on
`message`.

| Code   | Kind                  | Meaning                                            |
| ------ | --------------------- | -------------------------------------------------- |
| -32700 | `parse-error`         | Line was not valid JSON                            |
| -32600 | `invalid-request`     | Valid JSON, not a valid JSON-RPC request           |
| -32601 | `method-not-found`    | Unknown method                                     |
| -32602 | `invalid-params`      | Missing/invalid params (e.g. relative path)        |
| 1000   | `busy`                | Another engine job is running (dictation, retranscription, IPC, or maintenance) — retry later |
| 1001   | `setup-required`      | No model selected; user must finish app setup      |
| 1002   | `model-not-installed` | Selected model is not downloaded                   |
| 1003   | `model-load-failed`   | Model failed to load                               |
| 1004   | `unsupported-audio`   | Not a PCM WAV Handy can decode (or too long)       |
| 1005   | `file-not-found`      | Path does not exist                                |
| 1006   | `internal-error`      | Transcription failed                               |
| 1007   | `protocol-mismatch`   | Requested protocol version unsupported             |

## Concurrency

One engine job at a time, app-wide. Every path that touches the transcription
engine — dictation, history retranscription, IPC, model switching/unloading —
goes through a single reservation gate. Rules:

- A dictation session reserves the engine at *recording start* and holds it
  through transcription, so `transcribe.file` returns `busy` for the whole
  dictation — IPC never steals the engine from the user. The reservation is
  released as soon as the text is produced; LLM post-processing, history
  writes, and pasting do not block IPC.
- If any other job holds the engine — a history retranscription, another IPC
  call, a model switch — calls return `busy` immediately rather than queueing.
  Clients should retry with backoff.
- A dictation the user starts *during* an IPC transcription still records
  normally and takes a guaranteed handoff of the engine when that one job
  finishes — new background calls get `busy` in the meantime, so interactive
  dictation has strict priority over IPC clients.

## Reference client

A complete Rust reference client lives at
[`src-tauri/examples/ipc_client.rs`](../src-tauri/examples/ipc_client.rs) —
connect, handshake, call, and error handling in ~150 lines. Run it against a
running Handy:

```bash
cd src-tauri
cargo run --example ipc_client -- status
cargo run --example ipc_client -- transcribe /absolute/path/to/audio.wav
```

## Example client (Python)

```python
import json, socket, os

SOCK = os.path.expanduser(
    "~/Library/Application Support/com.pais.handy/handy.sock")  # macOS

s = socket.socket(socket.AF_UNIX)
s.connect(SOCK)
f = s.makefile("rw", encoding="utf-8", newline="\n")

def call(method, params=None, id=1):
    f.write(json.dumps({"jsonrpc": "2.0", "id": id,
                        "method": method, "params": params or {}}) + "\n")
    f.flush()
    return json.loads(f.readline())

print(call("status"))
print(call("transcribe.file", {"path": "/tmp/recording.wav"}))
```

Or from a shell (macOS/Linux, requires `nc` with Unix socket support):

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"status"}' | \
  nc -U "$HOME/Library/Application Support/com.pais.handy/handy.sock"
```

## Not in v1 (planned or under consideration)

- Streaming sessions (feed audio chunks, receive partial results)
- Request-scoped cancellation
- `model` parameter / model downloads over IPC
- CLI verbs on the `handy` binary backed by this socket
- MCP / Wyoming / HTTP facades

Implementation lives in `src-tauri/src/ipc/`; the service layer is
transport-independent so these can be added without breaking this contract.
