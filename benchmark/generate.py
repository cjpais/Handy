import os
import json
import base64
import urllib.request
import urllib.error
import struct
import time
import socket

# Load environment variables manually
def load_env(env_path):
    if not os.path.exists(env_path):
        return
    with open(env_path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            if "=" in line:
                key, val = line.split("=", 1)
                os.environ[key.strip()] = val.strip()

# Initialize directories
def init_directories():
    paths = [
        "benchmark/single_speaker/clean/malayalam_only",
        "benchmark/single_speaker/clean/code_switched",
        "benchmark/single_speaker/noisy/malayalam_only",
        "benchmark/single_speaker/noisy/code_switched",
        "benchmark/multi_speaker/clean/malayalam_only",
        "benchmark/multi_speaker/clean/code_switched",
        "benchmark/multi_speaker/noisy/malayalam_only",
        "benchmark/multi_speaker/noisy/code_switched"
    ]
    for path in paths:
        os.makedirs(path, exist_ok=True)

# Prepend WAV header to PCM data
def pcm_to_wav(pcm_data, sample_rate=24000, channels=1, bit_depth=16):
    num_bytes = len(pcm_data)
    byte_rate = int(sample_rate * channels * (bit_depth / 8))
    block_align = int(channels * (bit_depth / 8))
    
    header = struct.pack(
        '<4sI4s4sIHHIIHH4sI',
        b'RIFF',
        36 + num_bytes,
        b'WAVE',
        b'fmt ',
        16,
        1, # PCM format
        channels,
        sample_rate,
        byte_rate,
        block_align,
        bit_depth,
        b'data',
        num_bytes
    )
    return header + pcm_data

def get_audio_params(mime_type):
    rate = 24000
    channels = 1
    bit_depth = 16
    
    parts = mime_type.lower().split(";")
    for part in parts:
        part = part.strip()
        if part.startswith("rate="):
            try:
                rate = int(part.split("=")[1])
            except ValueError:
                pass
        elif part.startswith("channels="):
            try:
                channels = int(part.split("=")[1])
            except ValueError:
                pass
    return rate, channels, bit_depth

# Updated execution plan to route remaining 3.1 requests through Key 2
generation_plan = [
    # Single speaker, Malayalam only (Model: 2.5, Key: 2)
    {
        "file_id": "mal_single_01",
        "folder": "single_speaker/clean/malayalam_only",
        "text": "കേരളത്തിലെ കാലാവസ്ഥ വളരെ മനോഹരമാണ്.",
        "model_id": "gemini-2.5-flash-preview-tts",
        "key_index": 2, # Key 2
        "is_multi": False
    },
    {
        "file_id": "mal_single_02",
        "folder": "single_speaker/clean/malayalam_only",
        "text": "മലയാള ഭാഷ എനിക്ക് വളരെ ഇഷ്ടമാണ്.",
        "model_id": "gemini-2.5-flash-preview-tts",
        "key_index": 2,
        "is_multi": False
    },
    {
        "file_id": "mal_single_03",
        "folder": "single_speaker/clean/malayalam_only",
        "text": "നാളെ രാവിലെ നമുക്ക് തിരുവനന്തപുരത്തേക്ക് പോകാം.",
        "model_id": "gemini-2.5-flash-preview-tts",
        "key_index": 2,
        "is_multi": False
    },
    {
        "file_id": "mal_single_04",
        "folder": "single_speaker/clean/malayalam_only",
        "text": "ഈ ആപ്ലിക്കേഷൻ എങ്ങനെയാണ് ഉപയോഗിക്കേണ്ടത് എന്ന് പറയാമോ?",
        "model_id": "gemini-2.5-flash-preview-tts",
        "key_index": 2,
        "is_multi": False
    },
    {
        "file_id": "mal_single_05",
        "folder": "single_speaker/clean/malayalam_only",
        "text": "ഇന്നത്തെ വാർത്തകളിൽ പ്രധാനപ്പെട്ടത് എന്തൊക്കെയാണ്?",
        "model_id": "gemini-2.5-flash-preview-tts",
        "key_index": 2,
        "is_multi": False
    },

    # Single speaker, Code switched (Model: 3.1)
    {
        "file_id": "cs_single_01",
        "folder": "single_speaker/clean/code_switched",
        "text": "എന്റെ phone-ന്റെ battery തീരാറായി.",
        "model_id": "gemini-3.1-flash-tts-preview",
        "key_index": 1, # Key 1 (Generated successfully)
        "is_multi": False
    },
    {
        "file_id": "cs_single_02",
        "folder": "single_speaker/clean/code_switched",
        "text": "ആ project-ന്റെ deadline നാളെയാണെന്ന് ഓർമ്മയുണ്ടോ?",
        "model_id": "gemini-3.1-flash-tts-preview",
        "key_index": 1, # Key 1 (Generated successfully)
        "is_multi": False
    },
    {
        "file_id": "cs_single_03",
        "folder": "single_speaker/clean/code_switched",
        "text": "നാളെ നമുക്ക് ഒരു discussion ഉണ്ട്.",
        "model_id": "gemini-3.1-flash-tts-preview",
        "key_index": 1, # Key 1 (Generated successfully)
        "is_multi": False
    },
    {
        "file_id": "cs_single_04",
        "folder": "single_speaker/clean/code_switched",
        "text": "ഈ laptop വളരെ slow ആണ്, എനിക്ക് പുതിയത് വാങ്ങണം.",
        "model_id": "gemini-3.1-flash-tts-preview",
        "key_index": 1, # Key 1 (Generated successfully)
        "is_multi": False
    },
    {
        "file_id": "cs_single_05",
        "folder": "single_speaker/clean/code_switched",
        "text": "അവൻ നല്ലൊരു software engineer ആണ്.",
        "model_id": "gemini-3.1-flash-tts-preview",
        "key_index": 2, # Moved to Key 2 (Fresh quota)
        "is_multi": False
    },

    # Multi speaker, Malayalam only (Model: 3.1, Key: 2)
    {
        "file_id": "mal_multi_01",
        "folder": "multi_speaker/clean/malayalam_only",
        "text": "Speaker 1: ഹലോ, സുഖമാണോ? കുറെ നാളായല്ലോ കണ്ടിട്ട്.\nSpeaker 2: അതെ സുഖമാണ്! നീ എവിടെയായിരുന്നു ഇത്രയും നാൾ?",
        "model_id": "gemini-3.1-flash-tts-preview",
        "key_index": 2, # Key 2
        "is_multi": True
    },
    {
        "file_id": "mal_multi_02",
        "folder": "multi_speaker/clean/malayalam_only",
        "text": "Speaker 1: ഇന്നത്തെ കാലാവസ്ഥ എങ്ങനെയുണ്ട്? മഴ പെയ്യാൻ സാധ്യതയുണ്ടോ?\nSpeaker 2: ആകാശം കറുത്തിരുണ്ടിരിക്കുകയാണ്, എപ്പോൾ വേണമെങ്കിലും മഴ പെയ്യാം.",
        "model_id": "gemini-3.1-flash-tts-preview",
        "key_index": 2, # Key 2
        "is_multi": True
    },
    {
        "file_id": "mal_multi_03",
        "folder": "multi_speaker/clean/malayalam_only",
        "text": "Speaker 1: നാളത്തെ യാത്രയ്ക്കുള്ള ഒരുക്കങ്ങൾ ഒക്കെ കഴിഞ്ഞോ?\nSpeaker 2: അതെ, ബാഗ് എല്ലാം പാക്ക് ചെയ്തു. ടിക്കറ്റും റെഡിയാണ്.",
        "model_id": "gemini-3.1-flash-tts-preview",
        "key_index": 2, # Key 2
        "is_multi": True
    },

    # Multi speaker, Code switched (Model: 3.1, Key: 2)
    {
        "file_id": "cs_multi_01",
        "folder": "multi_speaker/clean/code_switched",
        "text": "Speaker 1: ഹലോ! പുതിയ project-ന്റെ status എന്തായി?\nSpeaker 2: എല്ലാം ready ആണ്, നാളെ client-ന് demo കാണിക്കാൻ പറ്റും.",
        "model_id": "gemini-3.1-flash-tts-preview",
        "key_index": 2, # Key 2
        "is_multi": True
    },
    {
        "file_id": "cs_multi_02",
        "folder": "multi_speaker/clean/code_switched",
        "text": "Speaker 1: എനിക്ക് എന്റെ password ഓർമ്മയില്ല. എങ്ങനെയെങ്കിലും reset ചെയ്യാമോ?\nSpeaker 2: അതെ, നിന്റെ email-ലേക്ക് വന്നിട്ടുള്ള link വഴി reset ചെയ്യാം.",
        "model_id": "gemini-3.1-flash-tts-preview",
        "key_index": 2, # Key 2
        "is_multi": True
    },
    {
        "file_id": "cs_multi_03",
        "folder": "multi_speaker/clean/code_switched",
        "text": "Speaker 1: നീ എപ്പോഴാണ് ഓഫീസ് വിടുന്നത്? നമുക്ക് ഒരുമിച്ച് coffee കുടിക്കാൻ പോകാം.\nSpeaker 2: ഒരു അര മണിക്കൂർ കൂടി കഴിഞ്ഞാൽ ഞാൻ free ആകും, അപ്പോൾ പോകാം.",
        "model_id": "gemini-3.1-flash-tts-preview",
        "key_index": 2, # Moved to Key 2
        "is_multi": True
    }
]

def generate_tts(text, model_id, api_key, is_multi=False):
    url = f"https://generativelanguage.googleapis.com/v1beta/models/{model_id}:generateContent?key={api_key}"
    
    # Prefix the prompt to force verbatim reading
    prefixed_text = f"Read the following transcript verbatim. Do not generate a conversational response.\n\nTranscript:\n{text}"
    
    if is_multi:
        speech_config = {
            "multiSpeakerVoiceConfig": {
                "speakerVoiceConfigs": [
                    {
                        "speaker": "Speaker 1",
                        "voiceConfig": {
                            "prebuiltVoiceConfig": {
                                "voiceName": "Puck"
                            }
                        }
                    },
                    {
                        "speaker": "Speaker 2",
                        "voiceConfig": {
                            "prebuiltVoiceConfig": {
                                "voiceName": "Zephyr"
                            }
                        }
                    }
                ]
            }
        }
    else:
        speech_config = {
            "voiceConfig": {
                "prebuiltVoiceConfig": {
                    "voiceName": "Aoede"
                }
            }
        }

    payload = {
        "contents": [
            {
                "parts": [
                    {
                        "text": prefixed_text
                    }
                ]
            }
        ],
        "generationConfig": {
            "responseModalities": ["audio"],
            "speechConfig": speech_config
        }
    }
    
    req = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
        method="POST"
    )
    
    # 90 seconds timeout
    with urllib.request.urlopen(req, timeout=90) as response:
        res_data = response.read().decode("utf-8")
        res_json = json.loads(res_data)
        
        candidates = res_json.get("candidates", [])
        if not candidates:
            raise Exception("No candidates returned")
            
        cand = candidates[0]
        finish_reason = cand.get("finishReason")
        
        content = cand.get("content", {})
        parts = content.get("parts", [])
        if not parts:
            raise Exception(f"No parts in content (finishReason: {finish_reason})")
            
        part = parts[0]
        if "inlineData" not in part:
            raise Exception(f"No inlineData in part: {part} (finishReason: {finish_reason})")
            
        inline_data = part["inlineData"]
        mime_type = inline_data.get("mimeType", "audio/l16")
        base64_data = inline_data.get("data", "")
        
        if not base64_data:
            raise Exception(f"Empty base64 data (finishReason: {finish_reason})")
            
        audio_bytes = base64.b64decode(base64_data)
        rate, channels, bit_depth = get_audio_params(mime_type)
        
        return pcm_to_wav(audio_bytes, sample_rate=rate, channels=channels, bit_depth=bit_depth)

# Wrapped with robust rate-limit and network timeout retry logic
def generate_tts_with_retry(text, model_id, api_key, is_multi=False):
    max_retries = 6
    backoff_delay = 5
    for attempt in range(max_retries):
        try:
            return generate_tts(text, model_id, api_key, is_multi)
        except urllib.error.HTTPError as e:
            error_body = e.read().decode("utf-8")
            if e.code == 429:
                print(f" [429 Rate Limit - Wait {backoff_delay}s]", end="", flush=True)
                time.sleep(backoff_delay)
                backoff_delay *= 2
                continue
            else:
                raise Exception(f"HTTP {e.code}: {error_body}")
        except (urllib.error.URLError, TimeoutError, socket.timeout) as e:
            print(f" [Timeout/Network Error - Wait {backoff_delay}s]", end="", flush=True)
            time.sleep(backoff_delay)
            backoff_delay *= 2
            continue
        except Exception as e:
            err_msg = str(e)
            if "429" in err_msg or "rate limit" in err_msg.lower() or "timed out" in err_msg.lower() or "timeout" in err_msg.lower():
                print(f" [Transient Error - Wait {backoff_delay}s]", end="", flush=True)
                time.sleep(backoff_delay)
                backoff_delay *= 2
                continue
            else:
                raise e
    raise Exception("Max retries exceeded due to rate limits or timeouts")

def main():
    load_env("benchmark/.env")
    init_directories()
    
    api_key_1 = os.getenv("GEMINI_API_KEY_1")
    api_key_2 = os.getenv("GEMINI_API_KEY_2")
    
    api_keys = {
        1: api_key_1,
        2: api_key_2
    }
    
    if not api_key_1 or not api_key_2:
        print("Error: Make sure both GEMINI_API_KEY_1 and GEMINI_API_KEY_2 are in benchmark/.env!", flush=True)
        return

    transcripts_map = {}
    
    for item in generation_plan:
        file_id = item["file_id"]
        folder = item["folder"]
        text = item["text"]
        model_id = item["model_id"]
        key_idx = item["key_index"]
        is_multi = item["is_multi"]
        
        current_key = api_keys[key_idx]
        
        clean_transcript = text.replace("Speaker 1:", "").replace("Speaker 2:", "").strip()
        transcripts_map[file_id] = {
            "original_text": text,
            "clean_text": clean_transcript,
            "is_multi_speaker": is_multi,
            "is_code_switched": "cs_" in file_id
        }
        
        wav_path = os.path.join("benchmark", folder, f"{file_id}.wav")
        txt_path = os.path.join("benchmark", folder, f"{file_id}.txt")
        
        # Resumable check: skip if files already exist
        if os.path.exists(wav_path) and os.path.exists(txt_path):
            print(f"Skipping {file_id}.wav (already generated)", flush=True)
            continue
            
        try:
            print(f"Generating {file_id}.wav using {model_id} (Key {key_idx})...", end="", flush=True)
            wav_data = generate_tts_with_retry(text, model_id, current_key, is_multi=is_multi)
            
            # Save WAV file
            with open(wav_path, "wb") as f:
                f.write(wav_data)
                
            # Save TXT file next to WAV file
            with open(txt_path, "w", encoding="utf-8") as f:
                f.write(clean_transcript)
                
            print(" [SAVED]", flush=True)
            
        except Exception as e:
            print(f" [CRITICAL ERROR: {e}]", flush=True)
            raise e
            
        # Sleep 5 seconds between requests to avoid rate limits
        time.sleep(5.0)
        
    # Write the transcripts map to file
    transcripts_json_path = "benchmark/transcripts.json"
    with open(transcripts_json_path, "w", encoding="utf-8") as f:
        json.dump(transcripts_map, f, ensure_ascii=False, indent=2)
    print(f"\nTranscripts successfully written to {transcripts_json_path}", flush=True)
    print("Clean audio generation complete!", flush=True)

if __name__ == "__main__":
    main()
