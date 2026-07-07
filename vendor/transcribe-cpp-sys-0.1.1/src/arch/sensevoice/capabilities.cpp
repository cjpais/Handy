// arch/sensevoice/capabilities.cpp - SenseVoice family-default
// capability values applied before read_capability_kv overlays the
// converter-emitted KV.

#include "sensevoice.h"

namespace transcribe::sensevoice {

void apply_family_invariants(transcribe_model & model) {
    transcribe_capabilities & caps = model.caps;

    // FunASR WavFrontend hard-codes 16 kHz; the upstream model card
    // rejects anything else. The CLI rejects non-16 kHz WAVs at load
    // time so this number is the contract.
    caps.native_sample_rate = 16000;

    // SenseVoice-Small does not advertise translation. The upstream
    // task heads emit language / event / emotion / ITN labels — not a
    // separate translation stream.
    caps.supports_translate = false;

    // The non-AR CTC head has no segment- or word-timestamp head in
    // the published runtime. forced-CTC alignment IS available via
    // ctc_forced_align in the upstream demo, but the C++ runtime does
    // not expose it; mark NONE.
    caps.max_timestamp_kind = TRANSCRIBE_TIMESTAMPS_NONE;

    // Feature bits: cancellation is wired at the run level. SenseVoice
    // exposes a runtime ITN toggle via the textnorm prefix embedding
    // (`woitn` / `withitn`); the generic transcribe_run_params::itn enum
    // routes here. No PNC runtime toggle.
    transcribe::set_feature(&model, TRANSCRIBE_FEATURE_CANCELLATION, true);
    transcribe::set_feature(&model, TRANSCRIBE_FEATURE_ITN, true);
}

}  // namespace transcribe::sensevoice
