// transcribe-loader.cpp - implementation of the GGUF loader.
//
// See transcribe-loader.h for the contract. This file is intentionally
// architecture-agnostic: it knows nothing about Parakeet, Whisper, or any
// other family. It only reads enough KV to identify which family handler
// the per-arch dispatch should hand the file to.

#include "transcribe-loader.h"

#include "gguf.h"
#include "transcribe-meta.h"
#include "transcribe-path.h"

namespace transcribe {

Loader::~Loader() {
    if (gguf_ != nullptr) {
        gguf_free(gguf_);
        gguf_ = nullptr;
    }
}

gguf_context * Loader::release_gguf() {
    gguf_context * out = gguf_;
    gguf_              = nullptr;
    return out;
}

transcribe_status Loader::open(const char * path) {
    if (path == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    path_ = path;

    // Distinguishes "the file is not at this path" (-> FILE_NOT_FOUND)
    // from every other reason gguf_init_from_file might return nullptr
    // (-> ERR_GGUF). Exact semantics in transcribe-path.h.
    if (!path_is_present(path)) {
        return TRANSCRIBE_ERR_FILE_NOT_FOUND;
    }

    // Header-only inspection: do not allocate ggml tensors. With ctx=null
    // gguf_init_from_file skips the entire tensor-allocation block; with
    // no_alloc=true any future code path that does pass a ctx will not
    // read the data blob.
    gguf_init_params init_params{};
    init_params.no_alloc = true;
    init_params.ctx      = nullptr;

    gguf_ = gguf_init_from_file(path, init_params);
    if (gguf_ == nullptr) {
        // ggml has already logged a structured error via GGML_LOG_ERROR
        // (corrupt magic, version mismatch, truncated header, IO error
        // after the existence check, etc.). All of those collapse to a
        // single public status.
        return TRANSCRIBE_ERR_GGUF;
    }

    // general.architecture is required. Without it the dispatch layer
    // has nothing to look up in the registry. Both Absent and BadType
    // are fatal: a missing arch and an arch with the wrong GGUF type
    // both leave us with no family to dispatch to.
    switch (read_string_kv(gguf_, "general.architecture", arch_)) {
        case KvResult::Absent:
        case KvResult::BadType:
            return TRANSCRIBE_ERR_GGUF;
        case KvResult::Ok:
            break;
    }

    // stt.variant is optional in 0.x. Per-family handlers default it
    // when absent (e.g. parakeet -> tdt-0.6b-v2). A present-but-wrong-
    // type variant is still a converter bug, though, so BadType is
    // fatal here while Absent silently leaves variant_ empty.
    switch (read_string_kv(gguf_, "stt.variant", variant_)) {
        case KvResult::Absent:
            // variant_ remains empty; family handler will default it.
            break;
        case KvResult::BadType:
            return TRANSCRIBE_ERR_GGUF;
        case KvResult::Ok:
            break;
    }

    // Copy every scalar-string KV into the metadata map. This mirrors
    // llama.cpp's generic metadata accessor: the public API exposes one
    // keyed getter (transcribe_model_meta_val_str) instead of a typed
    // accessor per field, so adding a new string KV in the converter needs
    // no API change. Non-string KVs (hparams, arrays such as the token
    // list) are intentionally skipped — this is identity/display metadata.
    const int64_t n_kv = gguf_get_n_kv(gguf_);
    for (int64_t i = 0; i < n_kv; ++i) {
        if (gguf_get_kv_type(gguf_, i) != GGUF_TYPE_STRING) {
            continue;
        }
        meta_.emplace(gguf_get_key(gguf_, i), gguf_get_val_str(gguf_, i));
    }

    return TRANSCRIBE_OK;
}

}  // namespace transcribe
