// arch/moonshine_streaming/capabilities.cpp - Moonshine-Streaming
// capability defaults.

#include "moonshine_streaming.h"

namespace transcribe::moonshine_streaming {

void apply_family_invariants(transcribe_model & model) {
    transcribe_capabilities & caps = model.caps;

    caps.native_sample_rate = 16000;

    // English-only model. Translation and language detection are off; the
    // family doc's Capability Validation table marks both SKIP — not
    // exposed by runtime.
    caps.supports_translate = false;

    // No timestamp tokens in the vocab; runtime returns a single
    // text-only segment with zeroed timings (same policy as moonshine).
    caps.max_timestamp_kind = TRANSCRIBE_TIMESTAMPS_NONE;

    // Streaming: stream_feed runs the encoder, adapter, and cross-KV
    // projection incrementally over a sliding window, appending each stable
    // slice to per-utterance host K/V buffers, and can run a throttled
    // partial AR decode over the growing cross-KV for a live transcript.
    // stream_finalize only tops up the trailing tail and runs a final AR
    // decode when frames advanced past the last partial (otherwise it just
    // commits the last partial transcript).
    caps.supports_streaming = true;

    // Streaming latency characteristics (≈240 ms cumulative encoder
    // right-context, natural 20 ms emit unit, family-recommended 80 ms
    // feed cadence) are documented in the family doc rather than
    // advertised as flat caps fields — the model has no inference-time
    // latency knob, and supports_streaming above is the generic gate.

    // Cancellation is wired at the per-run + per-feed level. No PNC/ITN
    // runtime toggle. Whisper-style fallback / long-form / prompt
    // features do not apply.
    transcribe::set_feature(&model, TRANSCRIBE_FEATURE_CANCELLATION, true);
}

}  // namespace transcribe::moonshine_streaming
