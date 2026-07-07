// arch/granite_nar/capabilities.cpp - family invariants.

#include "granite_nar.h"

namespace transcribe::granite_nar {

void apply_family_invariants(transcribe_model & model) {
    transcribe_capabilities & caps = model.caps;

    caps.native_sample_rate = 16000;

    // NAR variant is ASR only. The non-autoregressive editor does not
    // support translation or speaker diarization or word timestamps.
    caps.max_timestamp_kind = TRANSCRIBE_TIMESTAMPS_NONE;
    caps.supports_translate = false;

    // Cancellation is wired at the per-run level. No runtime toggles
    // for prompt / temperature / long-form / PNC / ITN on this variant.
    transcribe::set_feature(&model, TRANSCRIBE_FEATURE_CANCELLATION, true);
}

}  // namespace transcribe::granite_nar
