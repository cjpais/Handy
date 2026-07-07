// arch/voxtral_realtime/capabilities.cpp - family invariants.

#include "voxtral_realtime.h"

namespace transcribe::voxtral_realtime {

void apply_family_invariants(transcribe_model & model) {
    transcribe_capabilities & caps = model.caps;

    caps.native_sample_rate = 16000;

    // Streaming transcription only; no timestamp tokens, no translation
    // (the model advertises translation=false). Auto-language only.
    caps.max_timestamp_kind = TRANSCRIBE_TIMESTAMPS_NONE;
    caps.supports_translate = false;

    // Native streaming. stream_finalize runs the offline forward over the
    // accumulated buffer (byte-identical to transcribe_run); stream_feed
    // emits throttled tentative hypotheses. The configurable transcription
    // delay (num_delay_tokens) is exposed via the stream extension in
    // include/transcribe/voxtral_realtime.h.
    caps.supports_streaming = true;

    // Offline path uses 1-gram-lookup speculative decode (verify graph at
    // T = spec_k_drafts + 1). Streaming and batched paths are not yet
    // spec-enabled.
    caps.supports_spec_decode = true;

    transcribe::set_feature(&model, TRANSCRIBE_FEATURE_CANCELLATION, true);
}

}  // namespace transcribe::voxtral_realtime
