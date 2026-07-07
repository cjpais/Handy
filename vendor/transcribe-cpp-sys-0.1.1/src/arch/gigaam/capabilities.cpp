// arch/gigaam/capabilities.cpp - GigaAM capability defaults.
//
// Family-default capabilities applied before transcribe::read_capability_kv
// runs. KV present in the GGUF overrides; KV absent leaves the default
// in place. See arch/parakeet/capabilities.cpp for the long-form rationale.

#include "gigaam.h"

namespace transcribe::gigaam {

void apply_family_invariants(transcribe_model & model) {
    transcribe_capabilities & caps = model.caps;

    // GigaAM-v3's FeatureExtractor is a fixed 16 kHz mel bank for every
    // published variant. The CLI rejects non-16 kHz audio at load time.
    caps.native_sample_rate = 16000;

    // Monolingual Russian. No translate head on any variant.
    caps.supports_translate = false;

    // RNN-T / CTC heads emit one token per encoder frame the greedy
    // loop kept (or one per blank-collapsed CTC frame). The finest
    // honest timestamp resolution is per-token.
    caps.max_timestamp_kind = TRANSCRIBE_TIMESTAMPS_TOKEN;

    // Cancellation is honored at the top of each run() so callers can
    // short-circuit a long audio. No initial-prompt / temperature
    // fallback / long-form chunker; those belong to autoregressive
    // families. No runtime PNC/ITN toggle.
    transcribe::set_feature(&model, TRANSCRIBE_FEATURE_CANCELLATION, true);
}

}  // namespace transcribe::gigaam
