// arch/funasr_nano/capabilities.cpp - family invariants.

#include "funasr_nano.h"

namespace transcribe::funasr_nano {

void apply_family_invariants(transcribe_model & model) {
    transcribe_capabilities & caps = model.caps;

    caps.native_sample_rate = 16000;

    // Fun-ASR-Nano produces transcript text via a Qwen3 chat-format
    // response; no timestamp head is exposed. The CTC head shipped in
    // the checkpoint is auxiliary (and partial — last 3 blocks missing
    // from the .pt file), so it is intentionally skipped at convert.
    caps.max_timestamp_kind = TRANSCRIBE_TIMESTAMPS_NONE;

    caps.supports_translate = false;

    // Feature bits: cancellation is wired at the run level. FunASR-Nano
    // exposes a runtime ITN toggle via the LLM system-prompt suffix
    // ("，不进行文本规整" appended on itn=false, omitted on itn=true).
    // The generic transcribe_run_params::itn enum routes here. No PNC
    // runtime toggle.
    transcribe::set_feature(&model, TRANSCRIBE_FEATURE_CANCELLATION, true);
    transcribe::set_feature(&model, TRANSCRIBE_FEATURE_ITN, true);
}

}  // namespace transcribe::funasr_nano
