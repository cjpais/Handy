# Malayalam ASR Synthetic Benchmarking Dataset

This directory contains a synthetically generated Malayalam speech dataset for testing and benchmarking Speech-to-Text (ASR) systems.

The dataset was generated using the Google Gemini TTS APIs (`gemini-3.1-flash-tts-preview` and `gemini-2.5-flash-preview-tts`) and processed locally to include noisy variations.

---

## Directory Structure

```
benchmark/
├── .env                          # Gemini API keys (gitignored)
├── .gitignore                    # Git rules to exclude audio files and env keys
├── generate.py                   # Re-runable script to generate clean files
├── add_noise.py                  # Script to add background Gaussian noise
├── transcripts.json              # Master transcript mapping for evaluation
│
├── single_speaker/
│   ├── clean/
│   │   ├── malayalam_only/       # 10 Pure Malayalam speech files
│   │   └── code_switched/       # 5 English-Malayalam code-switched speech files
│   └── noisy/
│       ├── malayalam_only/       # 10 Noisy counterparts
│       └── code_switched/       # 5 Noisy counterparts
│
└── multi_speaker/
    ├── clean/
    │   └── malayalam_only/       # 3 Multi-speaker pure Malayalam dialogues
    └── noisy/
        └── malayalam_only/       # 3 Noisy multi-speaker dialogues
```

---

## Dataset Scale and Properties

- **Total Clean Files:** 18 utterances / dialogues
- **Total Noisy Files:** 18 utterances / dialogues (generated with local Gaussian noise augmentation)
- **Format:** Standard WAV (`.wav`), 24,000 Hz, 16-bit, Mono.
- **Transcripts:** For each `.wav` file, a corresponding `.txt` file containing the verbatim ground-truth transcript is saved next to it in the same directory (e.g. `mal_single_01.txt` next to `mal_single_01.wav`). A master mapping of all files is stored in `transcripts.json`.

---

## How to Run / Re-generate

### 1. Requirements

- Python 3.11+
- Urllib (standard library) and JSON (standard library)

### 2. Run Generation (Clean Audio)

To generate the clean audio files from the Gemini API, run:

```bash
python benchmark/generate.py
```

_Note: The script is fully resumable. If you encounter network errors or transient API limit errors, simply run it again and it will skip already generated files._

### 3. Add Noise Augmentation

To generate/refresh the noisy counterparts, run:

```bash
python benchmark/add_noise.py
```

This reads the files in the `clean/` directories and adds natural background white noise, outputting them in the corresponding `noisy/` subdirectories.
