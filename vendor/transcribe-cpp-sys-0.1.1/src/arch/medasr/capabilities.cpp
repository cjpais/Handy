// arch/medasr/capabilities.cpp - MedASR (Google LASR-CTC) capability defaults.
//
// Family-default capabilities applied before transcribe::read_capability_kv
// runs. KV present in the GGUF overrides; KV absent leaves the default
// in place. See arch/parakeet/capabilities.cpp for the long-form rationale.

#include "medasr.h"

namespace transcribe::medasr {

void apply_family_invariants(transcribe_model & model) {
    transcribe_capabilities & caps = model.caps;

    // 16 kHz mel bank, English-only medical-dictation ASR.
    caps.native_sample_rate = 16000;

    // Monolingual English. Listed languages are still informational; the
    // language-detect path is not exposed.
    caps.supports_language_detect = false;

    // No translation head on this checkpoint.
    caps.supports_translate = false;

    // Pure offline CTC; HF pipeline's chunk_length_s=20 is chunked offline
    // batching, NOT real-time streaming.
    caps.supports_streaming = false;

    // CTC emits one token per kept frame (post-collapse-repeats +
    // drop-blanks). The finest honest timestamp resolution is per-token.
    caps.max_timestamp_kind = TRANSCRIBE_TIMESTAMPS_TOKEN;

    // Cancellation is honored at the top of each run() so callers can
    // short-circuit a long audio.
    transcribe::set_feature(&model, TRANSCRIBE_FEATURE_CANCELLATION, true);
}

}  // namespace transcribe::medasr
