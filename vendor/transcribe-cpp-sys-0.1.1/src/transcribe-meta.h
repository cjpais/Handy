// transcribe-meta.h - shared GGUF KV reading helpers + post-load
// metadata reads (capabilities, languages).
//
// INTERNAL. The public C ABI knows nothing about gguf_context; these
// helpers are how loader code and family handlers read a gguf_context once
// it's open.
//
//   - KvResult: a tri-state {Absent, Ok, BadType} that distinguishes "key
//     not written" (Absent, usually fine for optional keys) from "written
//     with the wrong type" (BadType, a converter bug -> TRANSCRIBE_ERR_GGUF).
//   - read_*_kv low-level helpers: one per scalar / string / array KV type,
//     writing the parsed value on Ok and leaving the out param untouched
//     on Absent / BadType.
//   - read_capability_kv / read_languages_kv: post-load helpers family
//     handlers call after applying their own defaults; the schema is
//     shared across families.

#pragma once

#include "transcribe.h"

#include <cstdint>
#include <string>
#include <vector>

struct gguf_context;
struct transcribe_model;

namespace transcribe {

// Tri-state result for a single GGUF KV read. Consumers must handle
// BadType explicitly rather than fall back silently.
enum class KvResult {
    Absent,   // Key not present in the gguf.
    Ok,       // Key present, type matches expectation, value extracted.
    BadType,  // Key present but the GGUF type does not match. Always a
              // converter bug; callers should surface as TRANSCRIBE_ERR_GGUF.
};

// ---------------------------------------------------------------------------
// Low-level scalar / string / string-array readers
// ---------------------------------------------------------------------------
//
// Every helper takes the gguf_context, the key, and an out parameter.
// On Absent / BadType the out parameter is NOT modified, so callers can
// safely pre-populate a default and rely on the read leaving it alone
// when the key is missing.

KvResult read_string_kv(const gguf_context * ctx, const char * key, std::string & out);

KvResult read_bool_kv(const gguf_context * ctx, const char * key, bool & out);

KvResult read_uint32_kv(const gguf_context * ctx, const char * key, uint32_t & out);

KvResult read_int32_kv(const gguf_context * ctx, const char * key, int32_t & out);

KvResult read_float32_kv(const gguf_context * ctx, const char * key, float & out);

// Convenience: special-token id KV. The GGUF tokenizer schema has not
// standardized whether token ids are uint32 or int32 (different
// converters in the wider ecosystem use different types). This helper
// accepts either and casts to int. BadType only fires when the key is
// present and is neither uint32 nor int32.
KvResult read_token_id_kv(const gguf_context * ctx, const char * key, int & out);

// String array. The destination vector is cleared and replaced with
// the parsed contents only on Ok. On Absent / BadType the destination
// is left untouched (caller's responsibility to pre-populate or
// post-validate).
KvResult read_string_array_kv(const gguf_context * ctx, const char * key, std::vector<std::string> & out);

// int32 scalar array. Same Absent/Ok/BadType semantics as the other
// readers; the destination vector is replaced only on Ok. Used by the
// Parakeet TDT durations KV (`stt.parakeet.tdt.durations`); generic
// enough to live alongside the other low-level readers.
KvResult read_int32_array_kv(const gguf_context * ctx, const char * key, std::vector<int32_t> & out);

// ---------------------------------------------------------------------------
// Higher-level required / optional KV helpers
// ---------------------------------------------------------------------------
//
// These sit on top of the low-level readers and fold
// KvResult → transcribe_status, logging with a caller-supplied family tag.
//
// "Required" helpers: Absent and BadType both surface as
// TRANSCRIBE_ERR_GGUF. "Optional" helpers: Absent silently applies
// the caller's default; BadType is fatal (we control the converter).

transcribe_status read_required_u32_kv(const gguf_context * gguf,
                                       const char *         key,
                                       const char *         error_tag,
                                       int32_t &            out);

transcribe_status read_required_f32_kv(const gguf_context * gguf,
                                       const char *         key,
                                       const char *         error_tag,
                                       float &              out);

transcribe_status read_required_string_kv(const gguf_context * gguf,
                                          const char *         key,
                                          const char *         error_tag,
                                          std::string &        out);

transcribe_status read_optional_bool_kv(const gguf_context * gguf,
                                        const char *         key,
                                        const char *         error_tag,
                                        bool                 default_value,
                                        bool &               out);

transcribe_status read_optional_string_kv(const gguf_context * gguf,
                                          const char *         key,
                                          const char *         error_tag,
                                          const char *         default_value,
                                          std::string &        out);

transcribe_status read_optional_int32_kv(const gguf_context * gguf,
                                         const char *         key,
                                         const char *         error_tag,
                                         int32_t              default_value,
                                         int32_t &            out);

// ---------------------------------------------------------------------------
// Post-load shared metadata
// ---------------------------------------------------------------------------
//
// Post-load shared metadata population. The schema (stt.capability.*,
// general.languages) is shared across families by design.

// Read all recognized stt.capability.* boolean keys into caps, leaving
// fields untouched for absent keys. Call this AFTER family-default
// population so KV present overrides the default and KV absent keeps it.
// Recognized keys: stt.capability.{translate,lang_detect,streaming}.
//
// Returns:
//   TRANSCRIBE_OK              on success or "all keys absent".
//   TRANSCRIBE_ERR_INVALID_ARG if gguf is null.
//   TRANSCRIBE_ERR_GGUF        if any recognized key is present with
//                              the wrong type.
transcribe_status read_capability_kv(const gguf_context * gguf, transcribe_capabilities & caps);

// Read general.languages (string array of BCP-47-ish short codes) and
// install it on the model via transcribe_model::set_languages(); then
// likewise read optional stt.translation.target_languages and
// stt.translation.pairs into model-owned storage for the TRANSLATE target and
// pair gates. On Absent each list is left unchanged (the caller is expected to
// pre-populate it as zero / nullptr — see the "information gap, not a claim"
// comment in arch/parakeet/model.cpp).
//
// Returns:
//   TRANSCRIBE_OK              on success or absent.
//   TRANSCRIBE_ERR_INVALID_ARG if gguf is null.
//   TRANSCRIBE_ERR_GGUF        if a recognized array is present but is
//                              not a string array, or any element is null.
transcribe_status read_languages_kv(const gguf_context * gguf, transcribe_model & model);

}  // namespace transcribe
