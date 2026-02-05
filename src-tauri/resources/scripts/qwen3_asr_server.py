#!/usr/bin/env python3
"""
Qwen3 ASR Server Script using mlx-audio
Persistent server that keeps model loaded in memory for fast inference
"""

import sys
import json
import tempfile
import numpy as np
from pathlib import Path
import time

# 设置 HuggingFace 镜像（中国区可加速）
import os
os.environ['HF_ENDPOINT'] = 'https://hf-mirror.com'

# Global model cache
_stt_model = None
_model_name = "mlx-community/Qwen3-ASR-0.6B-8bit"

def load_model():
    """Load model once and cache it globally"""
    global _stt_model
    if _stt_model is None:
        from mlx_audio.stt import load as load_stt
        print(f"Loading Qwen3 ASR model: {_model_name}", file=sys.stderr, flush=True)
        start = time.time()
        _stt_model = load_stt(_model_name)
        load_time = time.time() - start
        print(f"Model loaded in {load_time:.2f}s", file=sys.stderr, flush=True)
    return _stt_model

def transcribe_audio(audio: np.ndarray, sample_rate: int = 16000, language: str = "auto") -> dict:
    """
    Transcribe audio using Qwen3 ASR with mlx-audio
    """
    try:
        from mlx_audio.audio_io import write as audio_write

        # Get cached model
        stt_model = load_model()

        # Save audio to temporary file (mlx-audio expects file path)
        with tempfile.NamedTemporaryFile(suffix='.wav', delete=False) as tmp_file:
            temp_path = tmp_file.name

        try:
            # Write audio data to temp file
            audio_write(temp_path, audio, sample_rate)

            # Determine language - Qwen3 supports multiple languages
            # Default to Chinese for better Chinese ASR support
            # Supported languages include: Chinese, English, Cantonese, Japanese, Korean, etc.
            if language in ["auto", "", None]:
                lang_param = "Chinese"  # Default to Chinese for better Chinese ASR
            else:
                # Map common language codes to Qwen3 language names
                lang_map = {
                    "zh": "Chinese",
                    "en": "English",
                    "ja": "Japanese",
                    "ko": "Korean",
                    "es": "Spanish",
                    "fr": "French",
                    "de": "German",
                    "it": "Italian",
                    "pt": "Portuguese",
                    "ru": "Russian",
                    "ar": "Arabic",
                    "hi": "Hindi",
                    "th": "Thai",
                    "vi": "Vietnamese",
                    "tr": "Turkish",
                    "pl": "Polish",
                    "nl": "Dutch",
                    "sv": "Swedish",
                    "da": "Danish",
                    "fi": "Finnish",
                    "cs": "Czech",
                    "el": "Greek",
                    "ro": "Romanian",
                    "hu": "Hungarian",
                }
                lang_lower = language.lower()
                lang_param = lang_map.get(lang_lower, language)

            # Run transcription with language parameter
            result_generator = stt_model.generate(temp_path, language=lang_param)

            # Extract text from result (handle generator)
            text = ""
            try:
                for chunk in result_generator:
                    if hasattr(chunk, 'text'):
                        text += chunk.text
                    elif isinstance(chunk, str):
                        text += chunk
            except Exception:
                if hasattr(result_generator, 'text'):
                    text = result_generator.text
                else:
                    text = str(result_generator)

            return {
                "text": text,
                "language": "auto",
                "confidence": 0.95
            }
        finally:
            # Clean up temp file
            Path(temp_path).unlink(missing_ok=True)

    except Exception as e:
        import traceback
        traceback.print_exc()
        return {
            "error": str(e),
            "text": f"[转录错误: {str(e)}]",
            "language": "auto",
            "confidence": 0.0
        }

def main():
    """Main entry point - persistent server mode"""
    print("Qwen3 ASR Server starting...", file=sys.stderr, flush=True)

    # Pre-load model on startup
    try:
        load_model()
        print("READY", flush=True)  # Signal ready to parent process
    except Exception as e:
        print(f"FAILED: {e}", file=sys.stderr, flush=True)
        sys.exit(1)

    # Process transcription requests
    while True:
        try:
            # Read line from stdin
            line = sys.stdin.readline()
            if not line:
                break  # EOF

            line = line.strip()
            if not line:
                continue

            # Parse request
            data = json.loads(line)

            # Parse audio data
            audio = np.array(data['audio'], dtype=np.float32)

            # Parse params
            params = data.get('params', {})
            if isinstance(params, str):
                params = json.loads(params)

            sample_rate = params.get('sample_rate', 16000)
            language = params.get('language', 'auto')

            # Run transcription with language support
            result = transcribe_audio(audio, sample_rate, language)

            # Output JSON result
            print(json.dumps(result), flush=True)

        except Exception as e:
            import traceback
            traceback.print_exc()
            error_response = {
                "error": str(e),
                "text": f"[处理错误: {str(e)}]",
                "language": "auto",
                "confidence": 0.0
            }
            print(json.dumps(error_response), flush=True)

if __name__ == "__main__":
    main()
