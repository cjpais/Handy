// arch/granite/capabilities.cpp - family invariants.

#include "granite.h"

namespace transcribe::granite {

void apply_family_invariants(transcribe_model & model) {
    transcribe_capabilities & caps = model.caps;

    caps.native_sample_rate = 16000;

    // Only -plus exposes word timestamps ("[T:N]" markers). WORD is the family
    // upper bound; granite::load() lowers it to NONE per variant from the GGUF's
    // stt.capability.word_timestamps.
    caps.max_timestamp_kind = TRANSCRIBE_TIMESTAMPS_WORD;

    // Translation is advertised on the 1b / 2b variants via a separate
    // chat-template prompt. The -plus variant drops the translation
    // capability (its model card lists ASR + speaker diarization, not
    // translation). The default here is true so the loader can lower
    // it per-variant from the GGUF KV stt.capability.translate.
    caps.supports_translate = true;

    // Cancellation is wired at the per-run level. Whisper-specific
    // long-form / temperature-fallback / initial-prompt features do
    // not apply to granite (the LLM produces a single greedy
    // transcript per utterance). No PNC/ITN runtime toggle.
    transcribe::set_feature(&model, TRANSCRIBE_FEATURE_CANCELLATION, true);
}

}  // namespace transcribe::granite
