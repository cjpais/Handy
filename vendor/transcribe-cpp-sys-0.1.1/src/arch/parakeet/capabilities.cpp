// arch/parakeet/capabilities.cpp - Parakeet capability defaults.
//
// Family-default capability population, applied before the KV reader
// (transcribe::read_capability_kv) runs: KV present overrides the
// default, KV absent leaves it in place. Variant identity is descriptive
// metadata, not a behavioral discriminator. native_sample_rate is set
// here rather than exposed via KV (fixed 16 kHz mel bank on every
// variant); supports_language_detect / supports_streaming come from KV;
// n_languages / languages come from general.languages via
// read_languages_kv (run by the family handler after this function).

#include "parakeet.h"

namespace transcribe::parakeet {

void apply_family_invariants(transcribe_model & model) {
    transcribe_capabilities & caps = model.caps;

    // Fixed 16 kHz mel bank (NeMo AudioToMelSpectrogramPreprocessor) on
    // every published variant; the CLI rejects non-16 kHz WAVs at load.
    caps.native_sample_rate = 16000;

    // No published Parakeet variant has a translate task head.
    caps.supports_translate = false;

    // TDT decode emits one token per kept encoder frame; run() converts
    // step_at_emit to per-token t0/t1 and aggregates to word/segment.
    // TOKEN is the finest the family can honestly produce.
    caps.max_timestamp_kind = TRANSCRIBE_TIMESTAMPS_TOKEN;

    // Abort callback honored at the top of each run.
    transcribe::set_feature(&model, TRANSCRIBE_FEATURE_CANCELLATION, true);
}

}  // namespace transcribe::parakeet
