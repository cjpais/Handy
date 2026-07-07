// arch/voxtral/capabilities.cpp - family invariants.

#include "voxtral.h"

namespace transcribe::voxtral {

void apply_family_invariants(transcribe_model & model) {
    transcribe_capabilities & caps = model.caps;

    caps.native_sample_rate = 16000;

    // Voxtral 2507 emits transcript / answer text only; no timestamp
    // tokens of any kind.
    caps.max_timestamp_kind = TRANSCRIBE_TIMESTAMPS_NONE;

    // Speech translation IS an advertised capability for 2507. It is not
    // a task token: translation, Q&A and summarization all go through the
    // mistral-common instruct template (audio + free-text instruction).
    // The runtime exposes it via --translate / --target-language (which
    // synthesize a translate instruction) and a general free-text prompt.
    // The GGUF's stt.capability.translate=true is read into supports_translate
    // by read_capability_kv at load; default it true here as a fallback.
    caps.supports_translate = true;

    // Per-run cancellation; the free-text prompt is wired via the
    // INITIAL_PROMPT feature on the Voxtral run extension.
    transcribe::set_feature(&model, TRANSCRIBE_FEATURE_CANCELLATION, true);
    transcribe::set_feature(&model, TRANSCRIBE_FEATURE_INITIAL_PROMPT, true);
}

}  // namespace transcribe::voxtral
