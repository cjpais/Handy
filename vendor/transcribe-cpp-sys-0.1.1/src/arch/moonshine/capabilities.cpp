// arch/moonshine/capabilities.cpp - Moonshine ASR capability defaults.

#include "moonshine.h"

namespace transcribe::moonshine {

void apply_family_invariants(transcribe_model & model) {
    transcribe_capabilities & caps = model.caps;

    caps.native_sample_rate = 16000;

    // English-only model (per the model card and tokenizer vocab — no
    // language tokens, no <|translate|>). Translation and language
    // detection are off; the family doc's Capability Validation table
    // marks both SKIP — not exposed by runtime.
    caps.supports_translate = false;

    // Moonshine emits no timestamp tokens and the model card does not
    // advertise alignment. Mirror cohere's NONE policy: the runtime
    // returns a single text-only segment with zeroed timings.
    caps.max_timestamp_kind = TRANSCRIBE_TIMESTAMPS_NONE;

    // Cancellation is wired at the per-run level. No PNC/ITN toggle;
    // no whisper-style fallback / long-form / prompt features.
    transcribe::set_feature(&model, TRANSCRIBE_FEATURE_CANCELLATION, true);
}

}  // namespace transcribe::moonshine
