// transcribe-flash-policy.h - shared flash-attention policy helpers.
//
// INTERNAL, C++17. Not part of the public ABI.
//
// Flash-attention support in ggml is per-backend and per-head-dim, so the
// default on/off decision is per-family (it depends on head dim and the
// backend it was loaded on). What lives here is the one cross-family
// concern: honoring the TRANSCRIBE_NO_FLASH / TRANSCRIBE_FORCE_FLASH global
// debug overrides.

#pragma once

namespace transcribe::flash {

// Apply the TRANSCRIBE_NO_FLASH / TRANSCRIBE_FORCE_FLASH environment
// overrides to a pair of encoder/decoder flash flags. Either override forces
// both flags in the same direction; if both are set, FORCE wins. The flags
// are in/out: updated in-place if an override is set, left untouched
// otherwise.
void apply_env_overrides(bool & encoder_use_flash, bool & decoder_use_flash);

}  // namespace transcribe::flash
