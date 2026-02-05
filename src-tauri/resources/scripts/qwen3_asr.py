#!/usr/bin/env python3
"""
Qwen3 ASR Inference Script using mlx-audio
Runs on Apple Silicon macOS using the MLX framework
"""

import sys
import json
import tempfile
import numpy as np
from pathlib import Path

# 设置 HuggingFace 镜像（中国区可加速）
import os
os.environ['HF_ENDPOINT'] = 'https://hf-mirror.com'


def transcribe_audio(audio: np.ndarray, sample_rate: int = 16000) -> dict:
    """
    Transcribe audio using Qwen3 ASR with mlx-audio
    """
    try:
        # Import mlx-audio STT module
        from mlx_audio.stt import load as load_stt
        from mlx_audio.audio_io import write as audio_write

        # Qwen3 ASR model from mlx-community
        # Using 8-bit quantized model for efficiency
        model_name = "mlx-community/Qwen3-ASR-0.6B-8bit"

        print(f"Loading Qwen3 ASR model: {model_name}", file=sys.stderr)

        # Load the STT model (will download on first run)
        stt_model = load_stt(model_name)

        # Save audio to temporary file (mlx-audio expects file path)
        with tempfile.NamedTemporaryFile(suffix='.wav', delete=False) as tmp_file:
            temp_path = tmp_file.name

        # Write audio data to temp file
        audio_write(temp_path, audio, sample_rate)

        print(f"Transcribing audio: {len(audio)} samples at {sample_rate}Hz", file=sys.stderr)

        # Run transcription
        result_generator = stt_model.generate(temp_path)

        # Clean up temp file
        Path(temp_path).unlink(missing_ok=True)

        # Extract text from result (handle generator)
        text = ""
        try:
            # Try to iterate through the generator
            for chunk in result_generator:
                if hasattr(chunk, 'text'):
                    text += chunk.text
                elif isinstance(chunk, str):
                    text += chunk
        except Exception as e:
            # If iteration fails, try to get text directly
            if hasattr(result_generator, 'text'):
                text = result_generator.text
            else:
                text = str(result_generator)

        return {
            "text": text,
            "language": "auto",  # Qwen3 ASR auto-detects language
            "confidence": 0.95
        }

    except ImportError as e:
        error_msg = f"mlx-audio not installed: {e}. Run: pip install mlx-audio"
        print(error_msg, file=sys.stderr)
        return {
            "error": error_msg,
            "text": f"[错误: mlx-audio 未安装。请运行: pip install mlx-audio]",
            "language": "auto",
            "confidence": 0.0
        }
    except Exception as e:
        import traceback
        traceback.print_exc()
        error_msg = str(e)
        print(f"Error during transcription: {error_msg}", file=sys.stderr)
        return {
            "error": error_msg,
            "text": f"[转录错误: {error_msg}]",
            "language": "auto",
            "confidence": 0.0
        }


def main():
    """Main entry point - reads audio from stdin"""
    try:
        # Read JSON input from stdin
        input_data = sys.stdin.read()
        data = json.loads(input_data)

        # Parse audio data
        audio = np.array(data['audio'], dtype=np.float32)

        # Parse params
        params = data.get('params', {})
        if isinstance(params, str):
            params = json.loads(params)

        # Get sample rate (default 16kHz)
        sample_rate = params.get('sample_rate', 16000)

        # Run transcription
        result = transcribe_audio(audio, sample_rate)

        # Output JSON result
        print(json.dumps(result))

    except Exception as e:
        import traceback
        traceback.print_exc()
        error_response = {
            "error": str(e),
            "text": f"[处理错误: {str(e)}]",
            "language": "auto",
            "confidence": 0.0
        }
        print(json.dumps(error_response))
        sys.exit(1)


if __name__ == "__main__":
    main()
