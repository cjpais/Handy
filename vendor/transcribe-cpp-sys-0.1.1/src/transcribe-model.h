// transcribe-model.h - internal base class for the public opaque
// transcribe_model handle.
//
// INTERNAL. The public C ABI forward-declares `struct transcribe_model;`;
// the real definition lives here so per-family code (src/arch/<family>/...)
// can derive from it. The base owns what is invariant across all families
// (dispatch token, variant/backend strings, capabilities) and has a virtual
// destructor so transcribe_model_free() can delete polymorphically;
// per-family subclasses own the gguf_context, weights, and decoder state
// (freed via RAII in the subclass destructor). The capabilities language
// pointer chain is centralized in set_languages().

#pragma once

#include "transcribe.h"

#include <cstdint>
#include <map>
#include <string>
#include <vector>

namespace transcribe {
struct Arch;
class Tokenizer;

// Transparent comparator keeps transcribe_model_meta_val_str allocation-free.
// Loader and model must use the same map type for assignment.
using MetaMap = std::map<std::string, std::string, std::less<>>;
}  // namespace transcribe

// Forward declaration for the resolved primary backend handle stored below.
// The full type comes from ggml-backend.h in the .cpp that fills it.
struct ggml_backend;
typedef struct ggml_backend * ggml_backend_t;

// Defined in the global namespace with the `struct` keyword to match the
// public C ABI forward declaration `struct transcribe_model;`.
struct transcribe_model {
    // Dispatch token. Set by per-family load() to the family's Arch
    // instance. The central dispatcher reads this for arch_string() and
    // for per-call dispatch (init_context, run).
    const transcribe::Arch * arch = nullptr;

    // Identification, both surfaced via the public string accessors.
    // variant is whatever the family decided (loader leaves it empty if
    // stt.variant was absent; the family supplies a default).
    std::string variant;

    // All scalar-string GGUF metadata KVs (general.*, stt.variant,
    // tokenizer.ggml.model, chat_template, ...), keyed by GGUF key. Copied
    // from the loader right after the per-family load() returns — the loader
    // outlives that call, so no family handler has to populate this. Exposed
    // read-only via transcribe_model_meta_val_str(); this mirrors llama.cpp's
    // single generic metadata accessor rather than a typed accessor per field.
    transcribe::MetaMap meta;

    // Runtime backend currently bound to this model. Empty string means
    // "no backend bound" (both pre-binding and the model == NULL case
    // collapse to that one rule). See the public header for full semantics.
    std::string backend;

    // The resolved primary compute backend this model runs on (the handle
    // that owns the weight buffer). Set by per-family load() right where it
    // sets `backend`, from BackendPlan::primary. Used by the public
    // transcribe_model_get_device() accessor to recover the device — and its
    // live memory — without exposing the per-family BackendPlan. nullptr
    // until a family binds it.
    ggml_backend_t primary_backend = nullptr;

    // Public capabilities. Per-family load() fills this in directly,
    // calling set_languages() for the languages chain. Immutable after
    // a successful load. Zero-initialized here; the capabilities
    // copy-out accessor overwrites struct_size with the caller's view
    // before returning, so the value of caps.struct_size on the model
    // is irrelevant to external observers.
    transcribe_capabilities caps{};

    // Backing store for the transcribe_model_supports() probe. One bit
    // per transcribe_feature value (0..63). Per-family load() opts in
    // by calling transcribe::set_feature(m, FEATURE, true) inside
    // apply_family_invariants. Zero means "not supported"; the probe
    // returns false for any feature whose bit isn't set, including
    // unknown enum values out of range.
    uint64_t feature_bits = 0;

    // Basis for the session-level limits query (transcribe_session_get_limits).
    // A hard-context-cap family fills this at load() — the same place it
    // computes caps.max_audio_ms — so the generic query in transcribe.cpp can
    // recompute the effective limits for any session n_ctx without a
    // per-family hook. Left zero by unbounded / soft-window families (which
    // have no decoder context cap): zero model_max_ctx => the query reports
    // effective_n_ctx 0 (unbounded), effective_max_audio_ms = caps.max_audio_ms
    // (the soft window, independent of n_ctx), and max_kv_bytes 0.
    // See docs/input-limits.md.
    struct LimitsBasis {
        // True for families whose decoder has a context window the session
        // n_ctx cap applies to (qwen3_asr, granite, voxtral, cohere, canary,
        // ...). When false the family has no decoder context cap and the
        // fields below stay zero.
        bool    has_context_cap        = false;
        // When true, effective_max_audio_ms comes straight from
        // caps.max_audio_ms rather than being derived from effective_n_ctx.
        // Set by families whose AUDIO bound is the encoder positional table
        // (cohere, canary), where the decoder context bounds the transcript
        // length, not how much audio fits — so the audio limit must not shrink
        // when the caller lowers n_ctx. effective_n_ctx / max_kv_bytes still
        // come from the decoder (n_ctx-sensitive). Left false by families
        // whose audio tokens consume the decoder context (qwen3_asr, granite,
        // voxtral, ...), where the audio bound legitimately scales with n_ctx.
        bool    audio_from_caps        = false;
        // The model's trained decoder context window, in tokens.
        int32_t model_max_ctx          = 0;
        // Representative non-audio prompt token overhead (chat affixes etc.).
        int32_t prompt_overhead        = 0;
        // Generation budget reserved when sizing the input bound.
        int32_t gen_reserve            = 0;
        // Milliseconds of 16 kHz audio per audio token (inverse encoder rate),
        // used to turn an audio-token budget into effective_max_audio_ms.
        double  ms_per_audio_token     = 0.0;
        // KV elements per context token (n_kv_heads * head_dim * n_layers * 2,
        // for K and V). The query multiplies by the session kv_type byte size
        // and effective_n_ctx to estimate max_kv_bytes.
        int64_t kv_elems_per_ctx_token = 0;
    } limits{};

    // Wall-clock load time in microseconds, captured by per-family
    // load() (start at function entry, stop just before *out_model
    // is set). Surfaced via transcribe_get_timings on any context
    // derived from this model — load time is a model-scoped fact.
    int64_t t_load_us = 0;

    // Explicitly defaulted because the deleted copy/move declarations
    // below otherwise suppress the implicit default constructor.
    transcribe_model() = default;
    virtual ~transcribe_model();

    transcribe_model(const transcribe_model &)             = delete;
    transcribe_model & operator=(const transcribe_model &) = delete;
    transcribe_model(transcribe_model &&)                  = delete;
    transcribe_model & operator=(transcribe_model &&)      = delete;

    // Optional accessor for an internal Tokenizer instance. The default
    // returns nullptr; per-family models that load a tokenizer override
    // it. Used by internal code (and tests) to inspect the vocabulary
    // without dragging the per-family layout into the central dispatch.
    virtual const transcribe::Tokenizer * tokenizer() const { return nullptr; }

    // Replace the languages list. Stores the strings inside the model
    // (so their c_str() lifetime is bound to the model lifetime), then
    // rebuilds the capabilities pointer chain that the public ABI
    // exposes via transcribe_capabilities::languages. Per-family load()
    // calls this once after deciding the language list.
    void set_languages(std::vector<std::string> langs);

    // Replace the translation-target language list (the set valid for
    // run_params::target_language under TRANSCRIBE_TASK_TRANSLATE). Same
    // model-owned-storage discipline as set_languages(): copies the
    // strings in and republishes caps.translate_target_languages + count.
    void set_translate_target_languages(std::vector<std::string> langs);

    // Optional translation pair contract, stored as "src>dst" strings from
    // stt.translation.pairs. Empty means "not advertised" and leaves generic
    // pair validation inert.
    void set_translation_pairs(std::vector<std::string> pairs);
    bool allows_translation_pair(const char * src, const char * dst) const;

  private:
    // Backing storage for the languages chain. Kept private so the only
    // way to mutate it is through set_languages(), which guarantees the
    // capability struct's pointer + count stay in sync.
    std::vector<std::string>  language_storage_;
    std::vector<const char *> language_ptrs_;

    // Backing storage for the translation-target chain, mutated only via
    // set_translate_target_languages() for the same sync guarantee.
    std::vector<std::string>  translate_target_storage_;
    std::vector<const char *> translate_target_ptrs_;

    // Backing storage for optional generic translation pair validation.
    std::vector<std::string> translation_pair_storage_;
};

namespace transcribe {

// Internal feature-bit helpers. Per-family load() calls set_feature; the
// central probe (transcribe_model_supports) reads via has_feature. Both
// treat out-of-range values as no-ops / false so the bitset stays bounded.
//
// The feature parameter is int, NOT transcribe_feature, on purpose: a C
// caller can pass any int, and loading an out-of-range value through the
// enum type is UB in C++. Enum constants convert implicitly.
inline void set_feature(transcribe_model * m, int f, bool on) {
    if (m == nullptr) {
        return;
    }
    const unsigned bit = static_cast<unsigned>(f);
    if (bit >= 64) {
        return;
    }
    const uint64_t mask = (uint64_t) 1 << bit;
    if (on) {
        m->feature_bits |= mask;
    } else {
        m->feature_bits &= ~mask;
    }
}

inline bool has_feature(const transcribe_model * m, int f) {
    if (m == nullptr) {
        return false;
    }
    const unsigned bit = static_cast<unsigned>(f);
    if (bit >= 64) {
        return false;
    }
    return (m->feature_bits & ((uint64_t) 1 << bit)) != 0;
}

}  // namespace transcribe
