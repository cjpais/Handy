#!/usr/bin/env python3
"""Test script to verify MLX model download and functionality"""

import json
import sys
import os

def check_mlx_installed():
    """Check if mlx is installed"""
    print("=" * 60)
    print("Step 1: Checking if mlx is installed...")
    try:
        import mlx
        print(f"✓ MLX is installed")
        return True
    except ImportError as e:
        print(f"✗ MLX is not installed: {e}")
        print("  Install with: pip install mlx")
        return False

def check_mlx_audio_installed():
    """Check if mlx-audio is installed"""
    print("=" * 60)
    print("Step 2: Checking if mlx-audio is installed...")
    try:
        from mlx_audio.stt import load as load_stt
        print(f"✓ mlx-audio is installed")
        return True
    except ImportError as e:
        print(f"✗ mlx-audio is not installed: {e}")
        print("  Install with: pip install mlx-audio")
        return False

def check_model_cached():
    """Check if Qwen3 model is cached"""
    print("=" * 60)
    print("Step 3: Checking if Qwen3 model is cached...")

    home_dir = os.path.expanduser("~")
    mlx_cache_dir = os.path.join(home_dir, ".cache", "mlx_audio")

    print(f"MLX cache directory: {mlx_cache_dir}")

    if not os.path.exists(mlx_cache_dir):
        print(f"✗ Cache directory does not exist")
        return False

    # Check for Qwen3 model
    model_name = "mlx-community/Qwen3-ASR-0.6B-8bit"
    model_cache_dir = os.path.join(mlx_cache_dir, model_name.replace("/", "--"))

    print(f"Model cache directory: {model_cache_dir}")

    if not os.path.exists(model_cache_dir):
        print(f"✗ Model cache directory does not exist")
        return False

    print(f"✓ Model cache directory exists")

    # Check for essential files
    essential_files = ["model.safetensors", "config.json"]
    found_files = []

    for file in essential_files:
        file_path = os.path.join(model_cache_dir, file)
        if os.path.exists(file_path):
            size = os.path.getsize(file_path)
            print(f"  ✓ Found {file} ({size / 1024 / 1024:.2f} MB)")
            found_files.append(file)
        else:
            print(f"  ✗ Missing {file}")

    return len(found_files) > 0

def test_model_download():
    """Test downloading the model"""
    print("=" * 60)
    print("Step 4: Testing model loading (will download if not cached)...")

    # Set HuggingFace mirror for China region
    os.environ['HF_ENDPOINT'] = 'https://hf-mirror.com'
    print("Set HF_ENDPOINT=https://hf-mirror.com")

    try:
        from mlx_audio.stt import load as load_stt

        model_name = "mlx-community/Qwen3-ASR-0.6B-8bit"
        print(f"Loading model: {model_name}")
        print("This may take a while if the model needs to be downloaded...")
        print()

        # Load the model (this will download if not cached)
        model = load_stt(model_name)

        print(f"✓ Model loaded successfully!")
        print(f"  Model type: {type(model)}")

        return True

    except Exception as e:
        print(f"✗ Failed to load model: {e}")
        import traceback
        traceback.print_exc()
        return False

def test_transcription():
    """Test transcription with the model"""
    print("=" * 60)
    print("Step 5: Testing transcription...")

    try:
        from mlx_audio.stt import load as load_stt
        import numpy as np

        model_name = "mlx-community/Qwen3-ASR-0.6B-8bit"
        model = load_stt(model_name)

        # Create a simple test audio (1 second of silence)
        print("Creating test audio...")
        sample_rate = 16000
        duration = 1  # 1 second
        audio = np.zeros(int(sample_rate * duration), dtype=np.float32)

        # Save to temp file
        import tempfile
        import wave

        temp_file = tempfile.NamedTemporaryFile(suffix=".wav", delete=False)
        temp_file.close()

        with wave.open(temp_file.name, 'w') as wav_file:
            wav_file.setnchannels(1)
            wav_file.setsampwidth(2)
            wav_file.setframerate(sample_rate)
            # Convert float to int16
            audio_int16 = (audio * 32767).astype(np.int16)
            wav_file.writeframes(audio_int16.tobytes())

        print(f"Test audio saved to: {temp_file.name}")

        # Run transcription
        print("Running transcription...")
        result = model.generate(temp_file.name)

        print(f"✓ Transcription completed!")
        print(f"  Result: {result}")

        # Clean up
        os.unlink(temp_file.name)

        return True

    except Exception as e:
        print(f"✗ Failed to transcribe: {e}")
        import traceback
        traceback.print_exc()
        return False

def main():
    print("=" * 60)
    print("MLX Audio Model Download Test Script")
    print("=" * 60)
    print()

    # Run all tests
    results = []

    results.append(("MLX Installed", check_mlx_installed()))
    results.append(("MLX-Audio Installed", check_mlx_audio_installed()))
    results.append(("Model Cached", check_model_cached()))
    results.append(("Model Download", test_model_download()))
    # results.append(("Transcription", test_transcription()))

    # Summary
    print()
    print("=" * 60)
    print("Test Summary")
    print("=" * 60)

    for name, result in results:
        status = "✓ PASS" if result else "✗ FAIL"
        print(f"  {status}: {name}")

    all_passed = all(r[1] for r in results)

    print()
    if all_passed:
        print("All tests passed! ✓")
        return 0
    else:
        print("Some tests failed. ✗")
        return 1

if __name__ == "__main__":
    sys.exit(main())
