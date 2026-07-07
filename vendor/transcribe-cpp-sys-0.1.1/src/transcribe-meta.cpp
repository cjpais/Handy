// transcribe-meta.cpp - implementation of the shared GGUF KV helpers
// and post-load metadata readers declared in transcribe-meta.h.
//
// This file is the only place outside transcribe-loader.cpp and
// transcribe-tokenizer.cpp that talks to gguf.h directly. The helpers
// here are deliberately small: each one wraps one gguf_get_* call and
// folds the "key absent / wrong type / wrong array element type"
// outcomes into a single KvResult so the caller can handle each
// outcome explicitly.

#include "transcribe-meta.h"

#include "gguf.h"
#include "transcribe-log.h"
#include "transcribe-model.h"

#include <cstring>
#include <utility>

namespace transcribe {

// ---------------------------------------------------------------------------
// Low-level scalar / string / string-array readers
// ---------------------------------------------------------------------------

KvResult read_string_kv(const gguf_context * ctx, const char * key, std::string & out) {
    const int64_t key_id = gguf_find_key(ctx, key);
    if (key_id < 0) {
        return KvResult::Absent;
    }
    if (gguf_get_kv_type(ctx, key_id) != GGUF_TYPE_STRING) {
        return KvResult::BadType;
    }
    const char * v = gguf_get_val_str(ctx, key_id);
    if (v == nullptr) {
        // Defensive: gguf currently asserts on missing values, but if
        // upstream ever softens that we still want to refuse a null
        // pointer rather than constructing an empty string and
        // pretending the read succeeded.
        return KvResult::BadType;
    }
    out = v;
    return KvResult::Ok;
}

KvResult read_bool_kv(const gguf_context * ctx, const char * key, bool & out) {
    const int64_t key_id = gguf_find_key(ctx, key);
    if (key_id < 0) {
        return KvResult::Absent;
    }
    if (gguf_get_kv_type(ctx, key_id) != GGUF_TYPE_BOOL) {
        return KvResult::BadType;
    }
    out = gguf_get_val_bool(ctx, key_id);
    return KvResult::Ok;
}

KvResult read_uint32_kv(const gguf_context * ctx, const char * key, uint32_t & out) {
    const int64_t key_id = gguf_find_key(ctx, key);
    if (key_id < 0) {
        return KvResult::Absent;
    }
    if (gguf_get_kv_type(ctx, key_id) != GGUF_TYPE_UINT32) {
        return KvResult::BadType;
    }
    out = gguf_get_val_u32(ctx, key_id);
    return KvResult::Ok;
}

KvResult read_int32_kv(const gguf_context * ctx, const char * key, int32_t & out) {
    const int64_t key_id = gguf_find_key(ctx, key);
    if (key_id < 0) {
        return KvResult::Absent;
    }
    if (gguf_get_kv_type(ctx, key_id) != GGUF_TYPE_INT32) {
        return KvResult::BadType;
    }
    out = gguf_get_val_i32(ctx, key_id);
    return KvResult::Ok;
}

KvResult read_float32_kv(const gguf_context * ctx, const char * key, float & out) {
    const int64_t key_id = gguf_find_key(ctx, key);
    if (key_id < 0) {
        return KvResult::Absent;
    }
    if (gguf_get_kv_type(ctx, key_id) != GGUF_TYPE_FLOAT32) {
        return KvResult::BadType;
    }
    out = gguf_get_val_f32(ctx, key_id);
    return KvResult::Ok;
}

KvResult read_token_id_kv(const gguf_context * ctx, const char * key, int & out) {
    const int64_t key_id = gguf_find_key(ctx, key);
    if (key_id < 0) {
        return KvResult::Absent;
    }
    // Accept either uint32 or int32. Anything else is a converter
    // mistake — bool, float, string don't make sense for a token id.
    const gguf_type t = gguf_get_kv_type(ctx, key_id);
    if (t == GGUF_TYPE_UINT32) {
        out = static_cast<int>(gguf_get_val_u32(ctx, key_id));
        return KvResult::Ok;
    }
    if (t == GGUF_TYPE_INT32) {
        out = gguf_get_val_i32(ctx, key_id);
        return KvResult::Ok;
    }
    return KvResult::BadType;
}

KvResult read_string_array_kv(const gguf_context * ctx, const char * key, std::vector<std::string> & out) {
    const int64_t key_id = gguf_find_key(ctx, key);
    if (key_id < 0) {
        return KvResult::Absent;
    }
    if (gguf_get_kv_type(ctx, key_id) != GGUF_TYPE_ARRAY) {
        return KvResult::BadType;
    }
    if (gguf_get_arr_type(ctx, key_id) != GGUF_TYPE_STRING) {
        return KvResult::BadType;
    }

    const size_t             n = gguf_get_arr_n(ctx, key_id);
    std::vector<std::string> tmp;
    tmp.reserve(n);
    for (size_t i = 0; i < n; ++i) {
        const char * s = gguf_get_arr_str(ctx, key_id, i);
        if (s == nullptr) {
            // Treat a null inner pointer the same as a wrong type:
            // the converter produced something we cannot honor.
            return KvResult::BadType;
        }
        tmp.emplace_back(s);
    }
    out = std::move(tmp);
    return KvResult::Ok;
}

KvResult read_int32_array_kv(const gguf_context * ctx, const char * key, std::vector<int32_t> & out) {
    const int64_t key_id = gguf_find_key(ctx, key);
    if (key_id < 0) {
        return KvResult::Absent;
    }
    if (gguf_get_kv_type(ctx, key_id) != GGUF_TYPE_ARRAY) {
        return KvResult::BadType;
    }
    if (gguf_get_arr_type(ctx, key_id) != GGUF_TYPE_INT32) {
        return KvResult::BadType;
    }
    const size_t n    = gguf_get_arr_n(ctx, key_id);
    // gguf_get_arr_data returns a pointer to the contiguous flat data
    // for scalar arrays. We memcpy into the destination so the caller
    // owns its own storage and the gguf_context can be freed
    // independently. Empty arrays are accepted; the caller's
    // value-domain validation should reject them if a non-empty array
    // is required.
    const void * data = gguf_get_arr_data(ctx, key_id);
    if (data == nullptr && n > 0) {
        return KvResult::BadType;
    }
    std::vector<int32_t> tmp(n);
    if (n > 0) {
        std::memcpy(tmp.data(), data, n * sizeof(int32_t));
    }
    out = std::move(tmp);
    return KvResult::Ok;
}

// ---------------------------------------------------------------------------
// Higher-level required / optional KV helpers
// ---------------------------------------------------------------------------

transcribe_status read_required_u32_kv(const gguf_context * gguf,
                                       const char *         key,
                                       const char *         error_tag,
                                       int32_t &            out) {
    uint32_t v = 0;
    switch (read_uint32_kv(gguf, key, v)) {
        case KvResult::Ok:
            out = static_cast<int32_t>(v);
            return TRANSCRIBE_OK;
        case KvResult::Absent:
        case KvResult::BadType:
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: required KV \"%s\" missing or wrong type", error_tag, key);
            return TRANSCRIBE_ERR_GGUF;
    }
    return TRANSCRIBE_ERR_GGUF;
}

transcribe_status read_required_f32_kv(const gguf_context * gguf,
                                       const char *         key,
                                       const char *         error_tag,
                                       float &              out) {
    float v = 0.0f;
    switch (read_float32_kv(gguf, key, v)) {
        case KvResult::Ok:
            out = v;
            return TRANSCRIBE_OK;
        case KvResult::Absent:
        case KvResult::BadType:
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: required KV \"%s\" missing or wrong type", error_tag, key);
            return TRANSCRIBE_ERR_GGUF;
    }
    return TRANSCRIBE_ERR_GGUF;
}

transcribe_status read_required_string_kv(const gguf_context * gguf,
                                          const char *         key,
                                          const char *         error_tag,
                                          std::string &        out) {
    std::string v;
    switch (read_string_kv(gguf, key, v)) {
        case KvResult::Ok:
            out = std::move(v);
            return TRANSCRIBE_OK;
        case KvResult::Absent:
        case KvResult::BadType:
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: required KV \"%s\" missing or wrong type", error_tag, key);
            return TRANSCRIBE_ERR_GGUF;
    }
    return TRANSCRIBE_ERR_GGUF;
}

transcribe_status read_optional_bool_kv(const gguf_context * gguf,
                                        const char *         key,
                                        const char *         error_tag,
                                        bool                 default_value,
                                        bool &               out) {
    bool tmp = default_value;
    switch (read_bool_kv(gguf, key, tmp)) {
        case KvResult::Absent:
            out = default_value;
            return TRANSCRIBE_OK;
        case KvResult::Ok:
            out = tmp;
            return TRANSCRIBE_OK;
        case KvResult::BadType:
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: optional KV \"%s\" has wrong type", error_tag, key);
            return TRANSCRIBE_ERR_GGUF;
    }
    return TRANSCRIBE_ERR_GGUF;
}

transcribe_status read_optional_string_kv(const gguf_context * gguf,
                                          const char *         key,
                                          const char *         error_tag,
                                          const char *         default_value,
                                          std::string &        out) {
    std::string v;
    switch (read_string_kv(gguf, key, v)) {
        case KvResult::Absent:
            out = default_value;
            return TRANSCRIBE_OK;
        case KvResult::Ok:
            out = std::move(v);
            return TRANSCRIBE_OK;
        case KvResult::BadType:
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: optional KV \"%s\" has wrong type", error_tag, key);
            return TRANSCRIBE_ERR_GGUF;
    }
    return TRANSCRIBE_ERR_GGUF;
}

transcribe_status read_optional_int32_kv(const gguf_context * gguf,
                                         const char *         key,
                                         const char *         error_tag,
                                         int32_t              default_value,
                                         int32_t &            out) {
    int32_t tmp = default_value;
    switch (read_int32_kv(gguf, key, tmp)) {
        case KvResult::Absent:
            out = default_value;
            return TRANSCRIBE_OK;
        case KvResult::Ok:
            out = tmp;
            return TRANSCRIBE_OK;
        case KvResult::BadType:
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: optional KV \"%s\" has wrong type", error_tag, key);
            return TRANSCRIBE_ERR_GGUF;
    }
    return TRANSCRIBE_ERR_GGUF;
}

// ---------------------------------------------------------------------------
// Post-load shared metadata
// ---------------------------------------------------------------------------

namespace {

// Helper used by read_capability_kv: read one bool capability KV with
// the strict-now contract. Absent leaves `field` alone (so the caller
// can pre-populate a family default), Ok updates it, BadType returns
// TRANSCRIBE_ERR_GGUF.
transcribe_status read_capability_bool(const gguf_context * gguf, const char * key, bool & field) {
    switch (read_bool_kv(gguf, key, field)) {
        case KvResult::Absent:
            return TRANSCRIBE_OK;
        case KvResult::Ok:
            return TRANSCRIBE_OK;
        case KvResult::BadType:
            return TRANSCRIBE_ERR_GGUF;
    }
    return TRANSCRIBE_ERR_GGUF;  // unreachable; placates -Wreturn-type
}

}  // namespace

transcribe_status read_capability_kv(const gguf_context * gguf, transcribe_capabilities & caps) {
    if (gguf == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // max_timestamp_kind is NOT read here: converters emit timestamp-capability
    // KVs (canary stt.capability.timestamps, granite stt.capability.word_timestamps),
    // but the ceiling is variant-specific and applied in each family's load().
    if (auto st = read_capability_bool(gguf, "stt.capability.translate", caps.supports_translate);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_capability_bool(gguf, "stt.capability.lang_detect", caps.supports_language_detect);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_capability_bool(gguf, "stt.capability.streaming", caps.supports_streaming);
        st != TRANSCRIBE_OK) {
        return st;
    }

    return TRANSCRIBE_OK;
}

transcribe_status read_languages_kv(const gguf_context * gguf, transcribe_model & model) {
    if (gguf == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    std::vector<std::string> langs;
    switch (read_string_array_kv(gguf, "general.languages", langs)) {
        case KvResult::Absent:
            // Information gap, not a claim that the model has no
            // languages. Caller has already pre-populated
            // (n_languages = 0, languages = nullptr).
            break;
        case KvResult::Ok:
            // set_languages copies the strings into the model so
            // their c_str() pointers stay valid for the model's
            // lifetime.
            model.set_languages(std::move(langs));
            break;
        case KvResult::BadType:
            return TRANSCRIBE_ERR_GGUF;
    }
    // Optional translation metadata. The model stores copies so public caps
    // pointers remain valid; absent keys leave old GGUFs permissive.
    std::vector<std::string> targets;
    switch (read_string_array_kv(gguf, "stt.translation.target_languages", targets)) {
        case KvResult::Absent:
            break;
        case KvResult::Ok:
            model.set_translate_target_languages(std::move(targets));
            break;
        case KvResult::BadType:
            return TRANSCRIBE_ERR_GGUF;
    }

    std::vector<std::string> pairs;
    switch (read_string_array_kv(gguf, "stt.translation.pairs", pairs)) {
        case KvResult::Absent:
            break;
        case KvResult::Ok:
            model.set_translation_pairs(std::move(pairs));
            break;
        case KvResult::BadType:
            return TRANSCRIBE_ERR_GGUF;
    }

    return TRANSCRIBE_OK;
}

}  // namespace transcribe
