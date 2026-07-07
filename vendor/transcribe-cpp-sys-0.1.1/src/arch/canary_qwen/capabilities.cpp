// arch/canary_qwen/capabilities.cpp - family invariants.

#include "canary_qwen.h"

namespace transcribe::canary_qwen {

void apply_family_invariants(transcribe_model & model) {
    transcribe_capabilities & caps = model.caps;

    caps.native_sample_rate = 16000;

    // SALM emits a chat-format Qwen3 response. No timestamp head; no
    // language conditioning (English-only training); no separate
    // initial-prompt path beyond the chat-template prefix.
    caps.max_timestamp_kind = TRANSCRIBE_TIMESTAMPS_NONE;

    caps.supports_translate = false;

    // Cancellation is wired at the per-run level. No PNC/ITN runtime
    // toggle, no temperature fallback, no long-form chunker — those
    // remain Whisper- / family-specific elsewhere.
    transcribe::set_feature(&model, TRANSCRIBE_FEATURE_CANCELLATION, true);
}

}  // namespace transcribe::canary_qwen
