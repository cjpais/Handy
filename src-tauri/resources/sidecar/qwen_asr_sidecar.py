#!/usr/bin/env python3
"""
Qwen3-ASR sidecar process for Handy.

Communicates with the Rust backend via stdin/stdout JSON protocol.
Uses mlx-audio for inference on Apple Silicon.

Protocol:
  Request:  {"command": "transcribe", "audio_path": "/tmp/audio.wav", "language": "Chinese", "system_prompt": "請使用繁體中文輸出"}
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
import types

MODEL_ID = "mlx-community/Qwen3-ASR-0.6B-8bit"

model = None
_original_build_prompt = None


def send_response(data: dict):
    """Send a JSON response to stdout."""
    line = json.dumps(data, ensure_ascii=False)
    sys.stdout.write(line + "\n")
    sys.stdout.flush()


def send_error(message: str):
    send_response({"ok": False, "error": message})


def load_model():
    global model, _original_build_prompt
    try:
        from mlx_audio.stt import load
        model = load(MODEL_ID)
        # Save the original _build_prompt for monkey-patching later
        _original_build_prompt = model._model._build_prompt
        send_response({"ok": True})
    except Exception as e:
        send_error(f"Failed to load model: {e}")


def _make_build_prompt_with_system(system_prompt: str):
    """Create a patched _build_prompt that injects a system prompt."""
    import mlx.core as mx

    def _build_prompt_with_system(self, num_audio_tokens, language="English"):
        supported = self.config.support_languages or []
        supported_lower = {lang.lower(): lang for lang in supported}
        lang_name = supported_lower.get(language.lower(), language)

        prompt = (
            f"<|im_start|>system\n{system_prompt}<|im_end|>\n"
            f"<|im_start|>user\n<|audio_start|>{'<|audio_pad|>' * num_audio_tokens}<|audio_end|><|im_end|>\n"
            f"<|im_start|>assistant\nlanguage {lang_name}<asr_text>"
        )

        input_ids = self._tokenizer.encode(prompt, return_tensors="np")
        return mx.array(input_ids)

    return _build_prompt_with_system


def handle_transcribe(request: dict):
    global model, _original_build_prompt
    if model is None:
        send_error("Model not loaded")
        return

    audio_path = request.get("audio_path")
    if not audio_path or not os.path.exists(audio_path):
        send_error(f"Audio file not found: {audio_path}")
        return

    language = request.get("language")
    # Qwen3-ASR requires an explicit language — no auto-detect support
    if language in (None, "", "auto"):
        language = "English"

    system_prompt = request.get("system_prompt")

    try:
        inner_model = model._model

        # Monkey-patch _build_prompt if system_prompt is provided
        if system_prompt:
            patched = _make_build_prompt_with_system(system_prompt)
            inner_model._build_prompt = types.MethodType(patched, inner_model)
        else:
            # Restore original
            inner_model._build_prompt = types.MethodType(_original_build_prompt.__func__, inner_model)

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
    finally:
        # Always restore original to avoid leaking state
        if _original_build_prompt is not None:
            inner_model._build_prompt = types.MethodType(_original_build_prompt.__func__, inner_model)


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
