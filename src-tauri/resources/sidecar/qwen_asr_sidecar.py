#!/usr/bin/env python3
"""
Qwen3-ASR sidecar process for Handy.

Communicates with the Rust backend via stdin/stdout JSON protocol.
Uses mlx-audio for inference on Apple Silicon.

Protocol:
  Request:  {"command": "transcribe", "audio_path": "/tmp/audio.wav", "language": "auto"}
  Response: {"ok": true, "text": "Hello world"}

  Request:  {"command": "load_model"}
  Response: {"ok": true}

  Request:  {"command": "health"}
  Response: {"ok": true, "model_loaded": true}

  Request:  {"command": "shutdown"}
  (process exits)
"""

import json
import sys
import os
import traceback

MODEL_ID = "mlx-community/Qwen3-ASR-0.6B-8bit"

model = None


def send_response(data: dict):
    """Send a JSON response to stdout."""
    line = json.dumps(data, ensure_ascii=False)
    sys.stdout.write(line + "\n")
    sys.stdout.flush()


def send_error(message: str):
    send_response({"ok": False, "error": message})


def load_model():
    global model
    try:
        from mlx_audio.stt import load
        model = load(MODEL_ID)
        send_response({"ok": True})
    except Exception as e:
        send_error(f"Failed to load model: {e}")


def handle_transcribe(request: dict):
    global model
    if model is None:
        send_error("Model not loaded")
        return

    audio_path = request.get("audio_path")
    if not audio_path or not os.path.exists(audio_path):
        send_error(f"Audio file not found: {audio_path}")
        return

    language = request.get("language")
    # "auto" or empty means let the model auto-detect
    if language in (None, "", "auto"):
        language = None

    try:
        result = model.generate(audio_path, language=language)
        text = result.text if hasattr(result, "text") else str(result)
        detected_lang = result.language if hasattr(result, "language") else None
        send_response({
            "ok": True,
            "text": text.strip(),
            "language": detected_lang,
        })
    except Exception as e:
        send_error(f"Transcription failed: {e}\n{traceback.format_exc()}")


def handle_health():
    send_response({"ok": True, "model_loaded": model is not None})


def main():
    # Signal readiness
    send_response({"ok": True, "status": "ready"})

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue

        try:
            request = json.loads(line)
        except json.JSONDecodeError as e:
            send_error(f"Invalid JSON: {e}")
            continue

        command = request.get("command")

        if command == "load_model":
            load_model()
        elif command == "transcribe":
            handle_transcribe(request)
        elif command == "health":
            handle_health()
        elif command == "shutdown":
            send_response({"ok": True, "status": "shutting_down"})
            break
        else:
            send_error(f"Unknown command: {command}")

    sys.exit(0)


if __name__ == "__main__":
    main()
