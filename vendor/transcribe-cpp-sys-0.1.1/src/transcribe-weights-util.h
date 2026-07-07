// transcribe-weights-util.h - shared GGUF tensor validation helpers.
//
// INTERNAL. C++17. Consumed only from other .cpp files inside src/.
// Not part of the public include/transcribe.h ABI.
//
// Every per-family weights.cpp needs to resolve tensor names from the
// loaded GGUF's ctx_meta and validate the type + shape matches what
// the family's encoder / decoder graph expects. The pattern is
// identical across families (parakeet and cohere both duplicated it
// verbatim with only the log-tag differing), so it lives here.
//
// Usage: the per-family weights.cpp defines a short GET_* macro
// wrapper that captures the family tag string:
//
//     #define PARAKEET_TAG "parakeet"
//     #define GET_F32(slot, name, ...) do { \
//         ggml_tensor * _t = transcribe::weights::find_tensor( \
//             ctx_meta, (name), {GGML_TYPE_F32}, {__VA_ARGS__}, \
//             PARAKEET_TAG); \
//         if (_t == nullptr) return TRANSCRIBE_ERR_GGUF; \
//         (slot) = _t; \
//     } while (0)
//
// The GET_* macros stay per-family so the family's log tag ends up in
// diagnostics, but the *type allowlists* are shared via the
// TRANSCRIBE_QUANT_LINEAR_TYPES / TRANSCRIBE_QUANT_CONV_TYPES macros
// below. Every family must accept the full allowlist: the set of types
// tools/transcribe-quantize emits is a project-wide policy, not a
// per-family one. See docs/tools/quantization.md.

#pragma once

#include "ggml.h"

#include <cstdint>
#include <cstdio>
#include <initializer_list>

// Shared quant allowlists. These are the ggml types that
// tools/transcribe-quantize may place into a released GGUF; every
// family's GET_LIN / GET_CONV macros must accept exactly this set so
// adding a preset to transcribe-quantize doesn't silently bypass a
// family's loader validation.
//
// Expanded at macro-use time inside a brace list passed to
// transcribe::weights::find_tensor(..., allowed_types, ...). Preprocessor
// expansion (rather than a constexpr array) keeps the existing
// find_tensor signature (std::initializer_list<ggml_type>) untouched.

#define TRANSCRIBE_QUANT_LINEAR_TYPES                                                                             \
    GGML_TYPE_F32, GGML_TYPE_F16, GGML_TYPE_BF16, GGML_TYPE_Q4_0, GGML_TYPE_Q4_1, GGML_TYPE_Q5_0, GGML_TYPE_Q5_1, \
        GGML_TYPE_Q8_0, GGML_TYPE_Q4_K, GGML_TYPE_Q5_K, GGML_TYPE_Q6_K

#define TRANSCRIBE_QUANT_CONV_TYPES GGML_TYPE_F32, GGML_TYPE_F16

namespace transcribe::weights {

// Resolve `name` in `ctx_meta` and verify that:
//   - the tensor exists,
//   - its type is in `allowed_types`,
//   - ne[0..n-1] matches `expected_ne` (where n = expected_ne.size()),
//   - ne[n..GGML_MAX_DIMS-1] are all 1 (no unexpected rank).
//
// On success returns the borrowed tensor pointer. On failure logs a
// human-readable diagnostic to stderr (prefixed with `error_tag`) and
// returns nullptr. The lookup is purely against `ctx_meta`; the source
// (a real GGUF on disk, or an in-memory legacy whisper.cpp .bin
// adapter) is invisible at this layer.
ggml_tensor * find_tensor(ggml_context *                   ctx_meta,
                          const char *                     name,
                          std::initializer_list<ggml_type> allowed_types,
                          std::initializer_list<int64_t>   expected_ne,
                          const char *                     error_tag);

// Format a per-layer tensor name. The caller passes a printf fmt
// containing exactly one %d; the layer index is substituted. Returns
// a pointer to a thread-local static buffer — the next call within the
// same thread invalidates the previous pointer. const char * (not
// std::string) so GET_* macro call sites pass straight to find_tensor.
// The 128-byte buffer is ample for any realistic layer name.
inline const char * lname(const char * fmt, int layer_idx) {
    thread_local char buf[128];
    std::snprintf(buf, sizeof(buf), fmt, layer_idx);
    return buf;
}

}  // namespace transcribe::weights
