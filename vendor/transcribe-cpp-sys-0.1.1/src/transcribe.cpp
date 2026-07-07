// transcribe.cpp - public C API entry points + central dispatch.
//
// What lives here:
//   - The C entry points declared in include/transcribe.h.
//   - The single dispatch site mapping an opened GGUF to its per-family
//     Arch handler (transcribe_model_load_file).
//   - The log-sink publication / emission helpers.
//   - The factory functions that initialize each public params struct.
//
// What does NOT live here: GGUF reading (transcribe-loader), per-family
// load / context / run code (src/arch/<family>/...), and ggml-specific
// code. The introspection accessors read the base structs in
// transcribe-model.h / transcribe-session.h directly; per-family code owns
// load(), init_context(), and run().

#include "transcribe.h"

#include "arch/whisper/bin_load.h"
#include "ggml-backend.h"
#include "ggml.h"  // ggml_log_set: route ggml diagnostics into our sink
#include "transcribe-abi.h"
#include "transcribe-arch.h"
#include "transcribe-backend.h"
#include "transcribe-loader.h"
#include "transcribe-log.h"
#include "transcribe-model.h"
#include "transcribe-path.h"
#include "transcribe-session.h"
#include "transcribe-tokenizer.h"
#include "transcribe/whisper.h"

#if defined(TRANSCRIBE_GGML_BACKEND_DL) && defined(_WIN32)
#    ifndef WIN32_LEAN_AND_MEAN
#        define WIN32_LEAN_AND_MEAN
#    endif
#    ifndef NOMINMAX
#        define NOMINMAX
#    endif
#    include <windows.h>
#elif defined(TRANSCRIBE_GGML_BACKEND_DL)
#    include <dlfcn.h>
#endif

#include <algorithm>
#include <atomic>
#include <climits>
#include <cmath>
#include <cstdarg>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <exception>
#include <filesystem>
#include <fstream>
#include <ios>
#include <mutex>
#include <new>
#include <set>
#include <string>
#include <vector>

// Public C ABI entry points must not leak C++ exceptions. The guarded
// paths below map bad_alloc to OOM, other exceptions to BACKEND, and make
// teardown best-effort/no-throw. MSVC builds use /EHs so exceptions thrown
// by ggml C-ABI frames remain catchable here.

namespace {

template <typename Fn> transcribe_status api_guard_status(const char * fn_name, Fn && body) {
    try {
        return body();
    } catch (const std::bad_alloc &) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: out of memory", fn_name);
        return TRANSCRIBE_ERR_OOM;
    } catch (const std::exception & e) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: caught backend exception: %s", fn_name, e.what());
        return TRANSCRIBE_ERR_BACKEND;
    } catch (...) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: caught unknown backend exception", fn_name);
        return TRANSCRIBE_ERR_BACKEND;
    }
}

// Value-returning variant: `on_throw` is the entry point's documented
// hard-failure value (0 devices, false, INT_MIN, ...).
template <typename T, typename Fn> T api_guard_value(const char * fn_name, T on_throw, Fn && body) {
    try {
        return body();
    } catch (const std::exception & e) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: caught backend exception: %s", fn_name, e.what());
        return on_throw;
    } catch (...) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: caught unknown backend exception", fn_name);
        return on_throw;
    }
}

// Teardown variant: log and swallow. Frees must never fail.
template <typename Fn> void api_guard_void(const char * fn_name, Fn && body) {
    try {
        body();
    } catch (const std::exception & e) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_WARN, "%s: caught backend exception during teardown: %s", fn_name,
                            e.what());
    } catch (...) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_WARN, "%s: caught unknown backend exception during teardown", fn_name);
    }
}

}  // namespace

// Status

// Parameter is `int` (not `transcribe_status`) so a caller passing an
// out-of-range value does not form a bogus enum object. See the comment
// above the declaration in include/transcribe.h.
extern "C" const char * transcribe_status_string(int status) {
    switch (status) {
        case TRANSCRIBE_OK:
            return "ok";
        case TRANSCRIBE_ERR_INVALID_ARG:
            return "invalid argument";
        case TRANSCRIBE_ERR_NOT_IMPLEMENTED:
            return "not implemented";
        case TRANSCRIBE_ERR_FILE_NOT_FOUND:
            return "file not found";
        case TRANSCRIBE_ERR_GGUF:
            return "gguf load error";
        case TRANSCRIBE_ERR_UNSUPPORTED_ARCH:
            return "unsupported architecture";
        case TRANSCRIBE_ERR_UNSUPPORTED_VARIANT:
            return "unsupported variant";
        case TRANSCRIBE_ERR_OOM:
            return "out of memory";
        case TRANSCRIBE_ERR_BACKEND:
            return "backend error";
        case TRANSCRIBE_ERR_SAMPLE_RATE:
            return "sample rate mismatch";
        case TRANSCRIBE_ERR_UNSUPPORTED_LANGUAGE:
            return "unsupported language";
        case TRANSCRIBE_ERR_UNSUPPORTED_TASK:
            return "unsupported task";
        case TRANSCRIBE_ERR_UNSUPPORTED_TIMESTAMPS:
            return "unsupported timestamp granularity";
        case TRANSCRIBE_ERR_ABORTED:
            return "aborted by callback";
        case TRANSCRIBE_ERR_BAD_STRUCT_SIZE:
            return "caller-owned struct missing or has bad struct_size";
        case TRANSCRIBE_ERR_UNSUPPORTED_PNC:
            return "model does not support runtime PNC control (reserved; not currently returned)";
        case TRANSCRIBE_ERR_UNSUPPORTED_ITN:
            return "model does not support runtime ITN control (reserved; not currently returned)";
        case TRANSCRIBE_ERR_INPUT_TOO_LONG:
            return "input audio too long for model context";
        case TRANSCRIBE_ERR_OUTPUT_TRUNCATED:
            return "output truncated: decode hit the context/generation cap before end-of-stream";
        default:
            return "unknown status";
    }
}

// Version

// TRANSCRIBE_COMMIT is stamped by the build (git rev-parse --short HEAD at
// configure time); fall back to "unknown" so the accessor never returns
// NULL. TRANSCRIBE_VERSION comes from <transcribe.h>.
#ifndef TRANSCRIBE_COMMIT
#    define TRANSCRIBE_COMMIT "unknown"
#endif

extern "C" const char * transcribe_version(void) {
    return TRANSCRIBE_VERSION;
}

extern "C" const char * transcribe_version_commit(void) {
    return TRANSCRIBE_COMMIT;
}

// Raw enum reads at the public ABI boundary
//
// C callers can store ANY int in an enum-typed ABI field; in C++ loading an
// out-of-range value through an enum-typed lvalue is UB (UBSan traps it). So
// every public entry point reads caller-supplied enum values as raw bytes
// first, validates the raw int, and only then converts to the enum type. All
// public enums are int-sized; the static_assert below pins one.

static inline int enum_field_raw(const void * field) {
    int raw;
    std::memcpy(&raw, field, sizeof(raw));
    return raw;
}

static_assert(sizeof(transcribe_backend_request) == sizeof(int),
              "public enums must be int-sized for raw boundary reads");

// ABI metadata: sizeof/alignof for the public structs, so a binding can
// verify its layout against this build. Keep these switches in sync with the
// transcribe_abi_struct enum.

extern "C" size_t transcribe_abi_struct_size(transcribe_abi_struct which) {
    switch (enum_field_raw(&which)) {
        case TRANSCRIBE_ABI_MODEL_LOAD_PARAMS:
            return sizeof(struct transcribe_model_load_params);
        case TRANSCRIBE_ABI_SESSION_PARAMS:
            return sizeof(struct transcribe_session_params);
        case TRANSCRIBE_ABI_RUN_PARAMS:
            return sizeof(struct transcribe_run_params);
        case TRANSCRIBE_ABI_STREAM_PARAMS:
            return sizeof(struct transcribe_stream_params);
        case TRANSCRIBE_ABI_CAPABILITIES:
            return sizeof(struct transcribe_capabilities);
        case TRANSCRIBE_ABI_TIMINGS:
            return sizeof(struct transcribe_timings);
        case TRANSCRIBE_ABI_SEGMENT:
            return sizeof(struct transcribe_segment);
        case TRANSCRIBE_ABI_WORD:
            return sizeof(struct transcribe_word);
        case TRANSCRIBE_ABI_TOKEN:
            return sizeof(struct transcribe_token);
        case TRANSCRIBE_ABI_STREAM_UPDATE:
            return sizeof(struct transcribe_stream_update);
        case TRANSCRIBE_ABI_STREAM_TEXT:
            return sizeof(struct transcribe_stream_text);
        case TRANSCRIBE_ABI_SESSION_LIMITS:
            return sizeof(struct transcribe_session_limits);
        case TRANSCRIBE_ABI_EXT:
            return sizeof(struct transcribe_ext);
        case TRANSCRIBE_ABI_BACKEND_DEVICE:
            return sizeof(struct transcribe_backend_device);
    }
    return 0;  // unknown id: "cannot verify", never a real size
}

extern "C" size_t transcribe_abi_struct_align(transcribe_abi_struct which) {
    switch (enum_field_raw(&which)) {
        case TRANSCRIBE_ABI_MODEL_LOAD_PARAMS:
            return alignof(struct transcribe_model_load_params);
        case TRANSCRIBE_ABI_SESSION_PARAMS:
            return alignof(struct transcribe_session_params);
        case TRANSCRIBE_ABI_RUN_PARAMS:
            return alignof(struct transcribe_run_params);
        case TRANSCRIBE_ABI_STREAM_PARAMS:
            return alignof(struct transcribe_stream_params);
        case TRANSCRIBE_ABI_CAPABILITIES:
            return alignof(struct transcribe_capabilities);
        case TRANSCRIBE_ABI_TIMINGS:
            return alignof(struct transcribe_timings);
        case TRANSCRIBE_ABI_SEGMENT:
            return alignof(struct transcribe_segment);
        case TRANSCRIBE_ABI_WORD:
            return alignof(struct transcribe_word);
        case TRANSCRIBE_ABI_TOKEN:
            return alignof(struct transcribe_token);
        case TRANSCRIBE_ABI_STREAM_UPDATE:
            return alignof(struct transcribe_stream_update);
        case TRANSCRIBE_ABI_STREAM_TEXT:
            return alignof(struct transcribe_stream_text);
        case TRANSCRIBE_ABI_SESSION_LIMITS:
            return alignof(struct transcribe_session_limits);
        case TRANSCRIBE_ABI_EXT:
            return alignof(struct transcribe_ext);
        case TRANSCRIBE_ABI_BACKEND_DEVICE:
            return alignof(struct transcribe_backend_device);
    }
    return 0;
}

namespace {

// Ordered rank for timestamp granularities (NONE < SEGMENT < WORD < TOKEN),
// used to compare a request against a family's advertised maximum. AUTO and
// any unknown value get rank -1.
int timestamp_rank(transcribe_timestamp_kind k) {
    switch (k) {
        case TRANSCRIBE_TIMESTAMPS_NONE:
            return 0;
        case TRANSCRIBE_TIMESTAMPS_SEGMENT:
            return 1;
        case TRANSCRIBE_TIMESTAMPS_WORD:
            return 2;
        case TRANSCRIBE_TIMESTAMPS_TOKEN:
            return 3;
        case TRANSCRIBE_TIMESTAMPS_AUTO:
            return -1;
    }
    return -1;
}

// Shared run-params validation used by transcribe_run and
// transcribe_stream_begin. Both call sites have already validated
// that session, session->model, and params are non-null. The translate-task
// rejection differs between the two (run mirrors supports_translate,
// streaming-begin rejects unconditionally in v1), so each caller
// applies its own translate check before reaching this helper.
transcribe_status validate_run_params_common(const transcribe_session * session, const transcribe_run_params * params) {
    // Raw-validate every enum field before its first enum-typed load (see
    // enum_field_raw). Once a field passes here, downstream typed reads —
    // including the per-family handlers' — are defined.
    switch (enum_field_raw(&params->task)) {
        case TRANSCRIBE_TASK_TRANSCRIBE:
        case TRANSCRIBE_TASK_TRANSLATE:
            break;
        default:
            return TRANSCRIBE_ERR_INVALID_ARG;
    }
    switch (enum_field_raw(&params->timestamps)) {
        case TRANSCRIBE_TIMESTAMPS_NONE:
        case TRANSCRIBE_TIMESTAMPS_AUTO:
        case TRANSCRIBE_TIMESTAMPS_SEGMENT:
        case TRANSCRIBE_TIMESTAMPS_WORD:
        case TRANSCRIBE_TIMESTAMPS_TOKEN:
            break;
        default:
            return TRANSCRIBE_ERR_INVALID_ARG;
    }
    switch (enum_field_raw(&params->pnc)) {
        case TRANSCRIBE_PNC_MODE_DEFAULT:
        case TRANSCRIBE_PNC_MODE_OFF:
        case TRANSCRIBE_PNC_MODE_ON:
            break;
        default:
            return TRANSCRIBE_ERR_INVALID_ARG;
    }
    switch (enum_field_raw(&params->itn)) {
        case TRANSCRIBE_ITN_MODE_DEFAULT:
        case TRANSCRIBE_ITN_MODE_OFF:
        case TRANSCRIBE_ITN_MODE_ON:
            break;
        default:
            return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (params->timestamps != TRANSCRIBE_TIMESTAMPS_AUTO) {
        const int req_rank = timestamp_rank(params->timestamps);
        const int max_rank = timestamp_rank(session->model->caps.max_timestamp_kind);
        if (req_rank > max_rank) {
            return TRANSCRIBE_ERR_UNSUPPORTED_TIMESTAMPS;
        }
    }
    if (params->language != nullptr && session->model->caps.n_languages > 0 &&
        session->model->caps.languages != nullptr) {
        bool found = false;
        for (int i = 0; i < session->model->caps.n_languages; ++i) {
            const char * entry = session->model->caps.languages[i];
            if (entry != nullptr && std::strcmp(entry, params->language) == 0) {
                found = true;
                break;
            }
        }
        if (!found) {
            return TRANSCRIBE_ERR_UNSUPPORTED_LANGUAGE;
        }
    }
    // Translation-target gate: for a TRANSLATE request naming a target
    // language, reject up front when the model advertises a target set
    // (n > 0) that does not include it. Mirrors the source-language check
    // above and returns the same code. An empty/unadvertised set (old
    // GGUFs, ASR-only models) makes this inert; family-level target/pair
    // checks (e.g. canary's pivot pairs) still apply on top.
    if (params->task == TRANSCRIBE_TASK_TRANSLATE && params->target_language != nullptr &&
        session->model->caps.n_translate_target_languages > 0 &&
        session->model->caps.translate_target_languages != nullptr) {
        bool found = false;
        for (int i = 0; i < session->model->caps.n_translate_target_languages; ++i) {
            const char * entry = session->model->caps.translate_target_languages[i];
            if (entry != nullptr && std::strcmp(entry, params->target_language) == 0) {
                found = true;
                break;
            }
        }
        if (!found) {
            return TRANSCRIBE_ERR_UNSUPPORTED_LANGUAGE;
        }
    }
    // Translation-pair gate: when a GGUF advertises exact src>dst pairs and
    // the caller supplied both sides, reject unsupported directions before
    // family dispatch. A missing source hint keeps this inert because most
    // families do not perform generic language detection here.
    if (params->task == TRANSCRIBE_TASK_TRANSLATE && params->language != nullptr &&
        params->target_language != nullptr &&
        !session->model->allows_translation_pair(params->language, params->target_language)) {
        return TRANSCRIBE_ERR_UNSUPPORTED_LANGUAGE;
    }
    return TRANSCRIBE_OK;
}

}  // namespace

// Logging
//
// Two-atomic log sink. Publication stores userdata (relaxed) then cb
// (release); emission acquire-loads cb and relaxed-loads userdata. The
// acquire/release pairing on cb is the happens-before edge the supported
// "install once at startup, before threads or models exist" model needs.
// Reconfiguration is unsupported (the (cb, userdata) pair can tear and an
// in-flight emitter may still hold the old sink); the only guarantee then is
// absence of data races, not correct semantics.
//
// Three sink states, classified from one acquire load of g_log_cb:
//   nullptr        never configured -> stderr default (emit_or_stderr)
//   sentinel       disabled via transcribe_log_set(NULL) -> drop everything
//   anything else  caller's callback -> route everything
// The sentinel distinguishes "asked for silence" from "never called us"
// without a second atomic whose pairing could tear.

namespace {

// Stored in g_log_cb in place of NULL when the host explicitly disables
// logging. Never invoked; only compared against.
void transcribe_log_cb_disabled(transcribe_log_level, const char *, void *) {}

std::atomic<transcribe_log_callback> g_log_cb{ nullptr };
std::atomic<void *>                  g_log_userdata{ nullptr };

// Map a ggml log level onto the transcribe enum. The numeric values do
// NOT coincide (vendored ggml: DEBUG=1 INFO=2 WARN=3 ERROR=4;
// transcribe.h: INFO=1 WARN=2 ERROR=3 DEBUG=4), so the pass-through
// layer owns an explicit mapping — exactly the responsibility the
// transcribe_log_level contract assigns to it. Unknown future levels
// degrade to INFO rather than being dropped.
transcribe_log_level transcribe_log_level_from_ggml(int level) {
    switch (level) {
        case GGML_LOG_LEVEL_NONE:
            return TRANSCRIBE_LOG_LEVEL_NONE;
        case GGML_LOG_LEVEL_DEBUG:
            return TRANSCRIBE_LOG_LEVEL_DEBUG;
        case GGML_LOG_LEVEL_INFO:
            return TRANSCRIBE_LOG_LEVEL_INFO;
        case GGML_LOG_LEVEL_WARN:
            return TRANSCRIBE_LOG_LEVEL_WARN;
        case GGML_LOG_LEVEL_ERROR:
            return TRANSCRIBE_LOG_LEVEL_ERROR;
        case GGML_LOG_LEVEL_CONT:
            return TRANSCRIBE_LOG_LEVEL_CONT;
        default:
            return TRANSCRIBE_LOG_LEVEL_INFO;
    }
}

void transcribe_ggml_log_bridge(enum ggml_log_level level, const char * text, void *);

}  // namespace

extern "C" void transcribe_log_set(transcribe_log_callback cb, void * userdata) {
    g_log_userdata.store(userdata, std::memory_order_relaxed);
    g_log_cb.store(cb != nullptr ? cb : &transcribe_log_cb_disabled, std::memory_order_release);
    // Route ggml's own diagnostics — above all the per-module dlopen
    // failures from dynamic backend loading, the signal a host needs to
    // see why an accelerator degraded to CPU — through the same sink.
    // Installed on configuration (not at library init) so a host that
    // never calls transcribe_log_set keeps ggml's stderr default
    // untouched. ggml_log_set itself is a plain global store; under the
    // supported install-once-at-startup model that is race-free.
    ggml_log_set(&transcribe_ggml_log_bridge, nullptr);
}

// Log callbacks have no error channel and may run from catch/noexcept paths.
// Treat a throwing callback as a host bug: drop the message, report to stderr.
static void transcribe_log_invoke(transcribe_log_callback cb,
                                  transcribe_log_level    level,
                                  const char *            msg,
                                  void *                  userdata) noexcept {
    try {
        cb(level, msg, userdata);
    } catch (...) {
        std::fprintf(stderr, "transcribe: log callback threw; message dropped: %s\n", msg);
    }
}

// Internal emission helper. Not part of the public ABI, not declared in
// any header. Used by transcribe_print_timings and the advisory-warn
// path; future logging from the loader / frontend / decode can call
// through here as well. Drops the message in both the never-configured
// and explicitly-disabled states.
static void transcribe_log_emit(transcribe_log_level level, const char * msg) {
    const auto cb = g_log_cb.load(std::memory_order_acquire);
    if (cb == nullptr || cb == &transcribe_log_cb_disabled) {
        return;
    }
    void * userdata = g_log_userdata.load(std::memory_order_relaxed);
    transcribe_log_invoke(cb, level, msg, userdata);
}

// Wrapper for messages we want surfaced even when the caller never
// configured a log sink: never-configured falls back to stderr so dev /
// CLI builds don't silently drop diagnostics, while an explicit
// transcribe_log_set(NULL, ...) silences them per the public contract.
// The single load (instead of emit-then-recheck) also closes the benign
// race where a concurrently installed callback could double-emit.
static void transcribe_log_emit_or_stderr(transcribe_log_level level, const char * msg) {
    const auto cb = g_log_cb.load(std::memory_order_acquire);
    if (cb == nullptr) {  // never configured
        std::fprintf(stderr, "%s\n", msg);
        return;
    }
    if (cb == &transcribe_log_cb_disabled) {  // explicit silence
        return;
    }
    transcribe_log_invoke(cb, level, msg, g_log_userdata.load(std::memory_order_relaxed));
}

namespace {

// ggml -> transcribe sink bridge, installed by transcribe_log_set.
// transcribe messages carry no trailing newline (the stderr fallback
// appends one), while ggml messages usually embed theirs — strip exactly
// one so a host callback sees a uniform convention. GGML_LOG_LEVEL_CONT
// fragments pass through as-is with level CONT; joining them is the
// host's choice (a fragment is still information).
void transcribe_ggml_log_bridge(enum ggml_log_level level, const char * text, void *) {
    if (text == nullptr) {
        return;
    }
    char buf[1024];
    std::snprintf(buf, sizeof(buf), "%s", text);
    const size_t n = std::strlen(buf);
    if (n > 0 && buf[n - 1] == '\n') {
        buf[n - 1] = '\0';
    }
    transcribe_log_emit_or_stderr(transcribe_log_level_from_ggml(level), buf);
}

}  // namespace

// Internal printf-style logger declared in transcribe-log.h. Renders into
// a bounded stack buffer and forwards to the stderr-fallback emitter, so
// library internals (including per-family run() drivers) reach the
// caller's installed log sink instead of writing raw stderr. See
// transcribe-log.h and docs/input-limits.md.
namespace transcribe {
void log_msg(transcribe_log_level level, const char * fmt, ...) {
    char    buf[1024];
    va_list ap;
    va_start(ap, fmt);
    std::vsnprintf(buf, sizeof(buf), fmt, ap);
    va_end(ap);
    transcribe_log_emit_or_stderr(level, buf);
}
}  // namespace transcribe

// Params init functions
//
// Each input params struct is initialized by zero-filling and stamping
// struct_size, which works because every field's documented default is its
// zero value. struct_size == 0 is NOT a defaults form (uninitialized stack
// memory could hit the zero case); callers reach defaults via NULL only.

extern "C" void transcribe_model_load_params_init(struct transcribe_model_load_params * p) {
    if (p == nullptr) {
        return;
    }
    std::memset(p, 0, sizeof(*p));
    p->struct_size = sizeof(*p);
}

extern "C" void transcribe_session_params_init(struct transcribe_session_params * p) {
    if (p == nullptr) {
        return;
    }
    std::memset(p, 0, sizeof(*p));
    p->struct_size = sizeof(*p);
}

extern "C" void transcribe_run_params_init(struct transcribe_run_params * p) {
    if (p == nullptr) {
        return;
    }
    std::memset(p, 0, sizeof(*p));
    p->struct_size   = sizeof(*p);
    p->spec_k_drafts  = -1;  // family default
    p->max_new_tokens = -1;  // family default
    // Default to AUTO ("richest the model supports", resolved per-family at
    // run time) rather than the memset NONE.
    p->timestamps    = TRANSCRIBE_TIMESTAMPS_AUTO;
}

extern "C" void transcribe_stream_params_init(struct transcribe_stream_params * p) {
    if (p == nullptr) {
        return;
    }
    std::memset(p, 0, sizeof(*p));
    p->struct_size = sizeof(*p);
}

// Output struct init functions
//
// Output structs are caller-allocated buffers the library writes into,
// bounded by min(caller struct_size, library size). Every output field's
// zero value means "absent / unknown / false / none", so a zeroed struct +
// struct_size is the correct empty state. struct_size == 0 is rejected by
// the accessors as a "you forgot to init the buffer" error.

extern "C" void transcribe_capabilities_init(struct transcribe_capabilities * p) {
    if (p == nullptr) {
        return;
    }
    std::memset(p, 0, sizeof(*p));
    p->struct_size = sizeof(*p);
}

extern "C" void transcribe_session_limits_init(struct transcribe_session_limits * p) {
    if (p == nullptr) {
        return;
    }
    std::memset(p, 0, sizeof(*p));
    p->struct_size = sizeof(*p);
}

extern "C" void transcribe_timings_init(struct transcribe_timings * p) {
    if (p == nullptr) {
        return;
    }
    std::memset(p, 0, sizeof(*p));
    p->struct_size = sizeof(*p);
}

extern "C" void transcribe_stream_update_init(struct transcribe_stream_update * p) {
    if (p == nullptr) {
        return;
    }
    std::memset(p, 0, sizeof(*p));
    p->struct_size = sizeof(*p);
}

extern "C" void transcribe_stream_text_init(struct transcribe_stream_text * p) {
    if (p == nullptr) {
        return;
    }
    std::memset(p, 0, sizeof(*p));
    p->struct_size = sizeof(*p);
}

extern "C" void transcribe_segment_init(struct transcribe_segment * p) {
    if (p == nullptr) {
        return;
    }
    std::memset(p, 0, sizeof(*p));
    p->struct_size = sizeof(*p);
}

extern "C" void transcribe_backend_device_init(struct transcribe_backend_device * p) {
    if (p == nullptr) {
        return;
    }
    std::memset(p, 0, sizeof(*p));
    p->struct_size = sizeof(*p);
}

extern "C" void transcribe_word_init(struct transcribe_word * p) {
    if (p == nullptr) {
        return;
    }
    std::memset(p, 0, sizeof(*p));
    p->struct_size = sizeof(*p);
}

extern "C" void transcribe_token_init(struct transcribe_token * p) {
    if (p == nullptr) {
        return;
    }
    std::memset(p, 0, sizeof(*p));
    p->struct_size = sizeof(*p);
}

// Whisper telemetry + run-extension init and chunk-trace accessors live in
// arch/whisper/public.cpp so this dispatcher stays family-agnostic. Whisper
// run knobs are in transcribe_whisper_run_ext (via run_params::family);
// sensevoice/funasr_nano use_itn and canary pnc use the generic run_params
// itn/pnc enums.

// Extension helpers

extern "C" transcribe_status transcribe_ext_check(const struct transcribe_ext * ext,
                                                  uint32_t                      expected_kind,
                                                  uint64_t                      min_size) {
    if (ext == nullptr) {
        return TRANSCRIBE_OK;
    }
    if (ext->size < sizeof(struct transcribe_ext)) {
        return TRANSCRIBE_ERR_BAD_STRUCT_SIZE;
    }
    if (ext->kind != expected_kind) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (ext->size < min_size) {
        return TRANSCRIBE_ERR_BAD_STRUCT_SIZE;
    }
    return TRANSCRIBE_OK;
}

extern "C" bool transcribe_model_accepts_ext_kind(const struct transcribe_model * model,
                                                  transcribe_ext_slot             slot,
                                                  uint32_t                        kind) {
    if (model == nullptr || model->arch == nullptr) {
        return false;
    }
    if (model->arch->accepts_ext_kind == nullptr) {
        return false;
    }
    return model->arch->accepts_ext_kind(model, slot, kind);
}

extern "C" bool transcribe_model_supports(const struct transcribe_model * model, transcribe_feature feature) {
    // Raw read before any enum-typed load (see enum_field_raw): an unknown
    // feature id from a C caller answers false, per the documented probe
    // contract, instead of being UB.
    return transcribe::has_feature(model, enum_field_raw(&feature));
}

namespace {

// Minimum struct_size accepted on each caller-owned input/output struct.
// Sized to the prefix the library currently relies on: any field the
// library writes on a given call path must lie inside this prefix. New
// fields appended at the end of the public struct without growing the
// library-side prefix do NOT raise this value.
#define TRANSCRIBE_FIELD_END(type, field) (offsetof(type, field) + sizeof(((type *) 0)->field))

constexpr size_t k_min_model_params_size            = TRANSCRIBE_FIELD_END(transcribe_model_load_params, gpu_device);
constexpr size_t k_min_context_params_size          = TRANSCRIBE_FIELD_END(transcribe_session_params, kv_type);
constexpr size_t k_min_run_params_size              = TRANSCRIBE_FIELD_END(transcribe_run_params, family);
constexpr size_t k_min_stream_params_size           = TRANSCRIBE_FIELD_END(transcribe_stream_params, family);
constexpr size_t k_stream_params_commit_policy_size = TRANSCRIBE_FIELD_END(transcribe_stream_params, commit_policy);
constexpr size_t k_stream_params_agreement_n_size =
    TRANSCRIBE_FIELD_END(transcribe_stream_params, stable_prefix_agreement_n);
constexpr size_t k_min_stream_update_size = TRANSCRIBE_FIELD_END(transcribe_stream_update, buffered_ms);
constexpr size_t k_stream_update_committed_changed_size =
    TRANSCRIBE_FIELD_END(transcribe_stream_update, committed_changed);
constexpr size_t k_stream_update_tentative_changed_size =
    TRANSCRIBE_FIELD_END(transcribe_stream_update, tentative_changed);
constexpr size_t k_min_stream_text_size    = TRANSCRIBE_FIELD_END(transcribe_stream_text, raw_tentative_start_bytes);
constexpr size_t k_min_capabilities_size   = TRANSCRIBE_FIELD_END(transcribe_capabilities, supports_streaming);
constexpr size_t k_min_session_limits_size = TRANSCRIBE_FIELD_END(transcribe_session_limits, max_kv_bytes);
constexpr size_t k_min_segment_size        = TRANSCRIBE_FIELD_END(transcribe_segment, text);
constexpr size_t k_min_word_size           = TRANSCRIBE_FIELD_END(transcribe_word, text);
constexpr size_t k_min_token_size          = TRANSCRIBE_FIELD_END(transcribe_token, text);
constexpr size_t k_min_timings_size        = TRANSCRIBE_FIELD_END(transcribe_timings, decode_ms);
constexpr size_t k_min_backend_device_size = TRANSCRIBE_FIELD_END(transcribe_backend_device, kind);
// k_min_whisper_chunk_trace_size lives in arch/whisper/public.cpp with
// the chunk-trace accessor that uses it.

#undef TRANSCRIBE_FIELD_END

// Size-aware ABI helpers (check_struct_size / check_input_struct_size /
// copy_out_prefix) live in transcribe-abi.h so per-family public
// accessors (arch/whisper/public.cpp) share one definition. Pull them
// into this TU's unqualified scope so existing call sites are unchanged.
using transcribe::check_input_struct_size;
using transcribe::check_struct_size;
using transcribe::copy_out_prefix;

static bool has_field(uint64_t struct_size, size_t field_end) {
    return struct_size >= static_cast<uint64_t>(field_end);
}

// Takes the RAW integer, not the enum: a C caller can store any int in the
// struct's enum-typed field, and in C++ loading an out-of-range value through
// the enum lvalue is UB (UBSan trips on it). Callers memcpy the bytes into an
// int and validate before the first enum-typed load.
static bool valid_stream_commit_policy(int policy) {
    switch (policy) {
        case TRANSCRIBE_STREAM_COMMIT_AUTO:
        case TRANSCRIBE_STREAM_COMMIT_ON_FINALIZE:
        case TRANSCRIBE_STREAM_COMMIT_STABLE_PREFIX:
            return true;
    }
    return false;
}

}  // namespace

// Backend modules and device discovery
//
// transcribe_init_backends loads ggml backend modules from ONE caller-named
// directory (a Python wheel's package dir, an app bundle, ...) via
// ggml_backend_load_all_from_path; it never widens the search. ggml tolerates
// every per-module failure (missing system deps -> dlopen fails -> module
// skipped), the ship-Vulkan-by-default degradation contract.
//
// NOTE: these definitions must sit at FILE scope, outside the anonymous
// namespace above. clang exports extern "C" functions defined inside an
// unnamed namespace; gcc gives them internal linkage, which strips them from
// the shared library on Linux (undefined references at link).

// ggml's MSVC timer (ggml_time_us/ms in ggml.c) divides QueryPerformanceCounter
// by a global frequency that ggml_time_init() fills in from
// QueryPerformanceFrequency. ggml_init() calls it on first use, but every
// family times its own load (`ggml_time_us()` at the top of <family>_load)
// BEFORE any ggml context exists — so on Windows/MSVC the divisor is still 0
// and model load faults with STATUS_INTEGER_DIVIDE_BY_ZERO (0xC0000094). POSIX
// ggml_time_* read clock_gettime directly (no divisor) so the missing init is
// silently harmless there; arm64 masks integer ÷0 as 0. ggml.h documents
// ggml_time_init() as "call this once at the beginning of the program" — do
// exactly that at the first public entry point.
static void ensure_ggml_time_init() {
    static std::once_flag once;
    std::call_once(once, [] { ggml_time_init(); });
}

#if defined(TRANSCRIBE_GGML_BACKEND_DL)
static std::filesystem::path library_self_dir() {
#    if defined(_WIN32)
    HMODULE    module = nullptr;
    const auto flags  = GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;
    if (!GetModuleHandleExW(flags, reinterpret_cast<LPCWSTR>(&transcribe_init_backends_default), &module)) {
        return {};
    }

    std::wstring path(1024, L'\0');
    for (;;) {
        const DWORD n = GetModuleFileNameW(module, path.data(), static_cast<DWORD>(path.size()));
        if (n == 0) {
            return {};
        }
        if (n < path.size()) {
            path.resize(n);
            break;
        }
        if (path.size() >= 32768) {
            return {};
        }
        path.resize(path.size() * 2);
    }
    return std::filesystem::path(path).parent_path();
#    else
    Dl_info info{};
    if (dladdr(reinterpret_cast<const void *>(&transcribe_init_backends_default), &info) == 0 ||
        info.dli_fname == nullptr || info.dli_fname[0] == '\0') {
        return {};
    }
    std::filesystem::path path(info.dli_fname);
    std::error_code       ec;
    auto                  canonical = std::filesystem::weakly_canonical(path, ec);
    if (ec) {
        canonical = std::filesystem::absolute(path, ec);
    }
    if (ec) {
        canonical = path;
    }
    return canonical.parent_path();
#    endif
}

static std::string path_for_c_api(const std::filesystem::path & path) {
#    if defined(_WIN32)
    const auto u8 = path.u8string();
    return std::string(u8.begin(), u8.end());
#    else
    return path.string();
#    endif
}
#endif

static transcribe_status transcribe_init_backends_impl(const char * artifact_dir) {
    ensure_ggml_time_init();
    if (artifact_dir == nullptr || artifact_dir[0] == '\0') {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    {
        std::error_code ec;
        if (!std::filesystem::is_directory(transcribe::path_from_utf8(artifact_dir), ec)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "transcribe_init_backends: %s is not an existing directory",
                                artifact_dir);
            return TRANSCRIBE_ERR_FILE_NOT_FOUND;
        }
    }

    // Idempotency: loading the same directory twice would re-dlopen and
    // re-register every module (duplicate devices). Keyed on the canonical
    // path so two spellings of one directory count as one load.
    static std::mutex            s_mutex;
    static std::set<std::string> s_loaded_dirs;
    std::lock_guard<std::mutex>  lock(s_mutex);

    std::error_code       ec;
    std::filesystem::path canonical = std::filesystem::weakly_canonical(transcribe::path_from_utf8(artifact_dir), ec);
    const std::string     key       = ec ? std::string(artifact_dir) : canonical.u8string();
    if (s_loaded_dirs.find(key) == s_loaded_dirs.end()) {
        ggml_backend_load_all_from_path(artifact_dir);
        s_loaded_dirs.insert(key);

        // Post-scan device summary, through the installed sink only (release
        // ggml compiles its per-module load-failure diagnostics out, so this
        // line is the reliable signal of which backends made it). Callback-only
        // so a host without a sink gets no new import-time stderr noise.
        char         devices[256];
        size_t       off   = 0;
        const size_t n_dev = ggml_backend_dev_count();
        for (size_t i = 0; i < n_dev; ++i) {
            const int wrote = std::snprintf(devices + off, sizeof(devices) - off, "%s%s", i == 0 ? "" : ", ",
                                            ggml_backend_dev_name(ggml_backend_dev_get(i)));
            if (wrote < 0 || static_cast<size_t>(wrote) >= sizeof(devices) - off) {
                break;  // truncated; the prefix is summary enough
            }
            off += static_cast<size_t>(wrote);
        }
        char msg[512];
        std::snprintf(msg, sizeof(msg),
                      "transcribe_init_backends: %zu compute device(s) "
                      "registered after scanning %s: %s",
                      n_dev, artifact_dir, off > 0 ? devices : "(none)");
        transcribe_log_emit(TRANSCRIBE_LOG_LEVEL_INFO, msg);
    }

    if (ggml_backend_dev_count() == 0) {
        // Dynamic-backend build pointed at a directory with no usable
        // modules: the process has nothing to compute on. Loud and early
        // beats a confusing model-load failure later.
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "transcribe_init_backends: no compute devices registered after "
                            "loading %s (no usable ggml backend modules found there, and no "
                            "backends are compiled in)",
                            artifact_dir);
        return TRANSCRIBE_ERR_BACKEND;
    }
    return TRANSCRIBE_OK;
}

static transcribe_status transcribe_init_backends_default_impl(void) {
    ensure_ggml_time_init();
#if !defined(TRANSCRIBE_GGML_BACKEND_DL)
    return TRANSCRIBE_OK;
#else
    const auto dir = library_self_dir();
    if (dir.empty()) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "transcribe_init_backends_default: could not resolve the "
                            "directory containing libtranscribe");
        return TRANSCRIBE_ERR_BACKEND;
    }
    const auto s = path_for_c_api(dir);
    return transcribe_init_backends(s.c_str());
#endif
}

namespace {

// Map ggml's device-type enum onto the public transcribe_device_type. GPU
// and any unexpected type (e.g. the META tensor-parallel aggregate, which we
// never construct) collapse to GPU; the dedicated CPU/IGPU/ACCEL cases are
// reported faithfully.
transcribe_device_type to_device_type(enum ggml_backend_dev_type t) {
    switch (t) {
        case GGML_BACKEND_DEVICE_TYPE_CPU:
            return TRANSCRIBE_DEVICE_TYPE_CPU;
        case GGML_BACKEND_DEVICE_TYPE_IGPU:
            return TRANSCRIBE_DEVICE_TYPE_IGPU;
        case GGML_BACKEND_DEVICE_TYPE_ACCEL:
            return TRANSCRIBE_DEVICE_TYPE_ACCEL;
        case GGML_BACKEND_DEVICE_TYPE_GPU:
        default:
            return TRANSCRIBE_DEVICE_TYPE_GPU;
    }
}

// Fill a caller-owned transcribe_backend_device from a ggml device, honoring
// the caller's declared struct_size via copy_out_prefix. ggml_backend_dev_get_props
// queries memory live, so every call observes a fresh memory_free snapshot.
void fill_backend_device(ggml_backend_dev_t dev, uint64_t caller_size, struct transcribe_backend_device * out) {
    ggml_backend_dev_props props{};
    ggml_backend_dev_get_props(dev, &props);

    const transcribe::BackendKind kind      = transcribe::classify_device(dev);
    const char *                  kind_name = transcribe::kind_name(kind);
    const char *                  name      = ggml_backend_dev_name(dev);
    if (name == nullptr || name[0] == '\0') {
        name = (props.name != nullptr && props.name[0] != '\0') ? props.name : kind_name;
    }
    const char * description = ggml_backend_dev_description(dev);
    if (description == nullptr) {
        description = props.description != nullptr ? props.description : "";
    }

    struct transcribe_backend_device staged{};
    staged.struct_size  = caller_size;
    staged.name         = name;
    staged.description  = description;
    staged.kind         = kind_name;
    staged.device_id    = props.device_id;
    staged.memory_total = props.memory_total;
    staged.memory_free  = props.memory_free;
    staged.device_type  = to_device_type(props.type);
    copy_out_prefix(out, &staged, caller_size, sizeof(staged));
}

}  // namespace

static int transcribe_backend_device_count_impl(void) {
    return static_cast<int>(ggml_backend_dev_count());
}

static transcribe_status transcribe_get_backend_device_impl(int index, struct transcribe_backend_device * out) {
    if (out == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (const auto st = check_struct_size(out->struct_size, k_min_backend_device_size); st != TRANSCRIBE_OK) {
        return st;
    }
    if (index < 0 || index >= static_cast<int>(ggml_backend_dev_count())) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    ggml_backend_dev_t dev = ggml_backend_dev_get(static_cast<size_t>(index));
    fill_backend_device(dev, out->struct_size, out);
    return TRANSCRIBE_OK;
}

// Takes the request pre-read as a raw int (see enum_field_raw in the
// forwarder) so no enum-typed load of an unknown probe value ever happens;
// unknown values answer false per the documented probe contract.
static bool transcribe_backend_available_impl(int raw) {
    const size_t n = ggml_backend_dev_count();
    if (raw == TRANSCRIBE_BACKEND_AUTO) {
        return n > 0;
    }
    transcribe::BackendKind want;
    switch (raw) {
        case TRANSCRIBE_BACKEND_CPU:
        case TRANSCRIBE_BACKEND_CPU_ACCEL:
            want = transcribe::BackendKind::Cpu;
            break;
        case TRANSCRIBE_BACKEND_METAL:
            want = transcribe::BackendKind::Metal;
            break;
        case TRANSCRIBE_BACKEND_VULKAN:
            want = transcribe::BackendKind::Vulkan;
            break;
        case TRANSCRIBE_BACKEND_CUDA:
            want = transcribe::BackendKind::Cuda;
            break;
        default:
            return false;
    }
    for (size_t i = 0; i < n; ++i) {
        if (transcribe::classify_device(ggml_backend_dev_get(i)) == want) {
            return true;
        }
    }
    return false;
}

namespace {

enum class StreamStablePrefixImpl {
    GenericTextAgreement,
    FamilyTokenAgreement,
    FamilyNativeCommit,
};

static StreamStablePrefixImpl stream_stable_prefix_impl_for_arch(const transcribe::Arch * arch) {
    // Single source of truth for the stable-prefix implementation used by
    // AUTO and explicit STABLE_PREFIX. Add new streaming families here
    // deliberately; otherwise they use the generic text-agreement fallback.
    struct Entry {
        const char *           arch_name;
        StreamStablePrefixImpl impl;
    };

    static constexpr Entry k_defaults[] = {
        // Parakeet publishes native committed chunks; agreement_n does not
        // add useful evidence on top of the family-provided boundary.
        { "parakeet",            StreamStablePrefixImpl::FamilyNativeCommit   },
        // Moonshine re-decodes the full prefix and its family boundary is
        // token-id agreement; the family applies stable_prefix_agreement_n.
        { "moonshine_streaming", StreamStablePrefixImpl::FamilyTokenAgreement },
    };

    const char * name = arch != nullptr && arch->name != nullptr ? arch->name : "";
    for (const Entry & entry : k_defaults) {
        if (std::strcmp(name, entry.arch_name) == 0) {
            return entry.impl;
        }
    }
    return StreamStablePrefixImpl::GenericTextAgreement;
}

static size_t utf8_floor_boundary(const std::string & s, size_t pos) {
    if (pos >= s.size()) {
        return s.size();
    }
    while (pos > 0 && (static_cast<unsigned char>(s[pos]) & 0xC0u) == 0x80u) {
        --pos;
    }
    return pos;
}

static size_t common_prefix_bytes(const std::string & a, const std::string & b) {
    const size_t n = std::min(a.size(), b.size());
    size_t       i = 0;
    while (i < n && a[i] == b[i]) {
        ++i;
    }
    return utf8_floor_boundary(a, i);
}

static bool starts_with_bytes(const std::string & s, const std::string & prefix) {
    return s.size() >= prefix.size() && s.compare(0, prefix.size(), prefix) == 0;
}

static size_t token_prefix_raw_bytes(const transcribe_session * session, int n_tokens) {
    if (session == nullptr || n_tokens <= 0 || session->tokens.empty()) {
        return 0;
    }
    const int   capped = std::min<int>(n_tokens, static_cast<int>(session->tokens.size()));
    std::string prefix;
    prefix.reserve(session->full_text.size());
    for (int i = 0; i < capped; ++i) {
        prefix += session->tokens[static_cast<size_t>(i)].text;
    }
    if (starts_with_bytes(session->full_text, prefix)) {
        return prefix.size();
    }
    if (!prefix.empty() && prefix.front() == ' ' && (session->full_text.empty() || session->full_text.front() != ' ')) {
        const std::string normalized_prefix = prefix.substr(1);
        if (starts_with_bytes(session->full_text, normalized_prefix)) {
            return normalized_prefix.size();
        }
        return common_prefix_bytes(session->full_text, normalized_prefix);
    }
    return common_prefix_bytes(session->full_text, prefix);
}

static size_t family_candidate_raw_prefix_bytes(const transcribe_session * session) {
    if (session == nullptr || !session->has_result) {
        return 0;
    }
    if (session->n_committed_tokens > 0) {
        return token_prefix_raw_bytes(session, session->n_committed_tokens);
    }
    if (session->n_committed_words > 0 && session->n_committed_words >= static_cast<int>(session->words.size())) {
        return session->full_text.size();
    }
    if (session->n_committed_segments > 0 &&
        session->n_committed_segments >= static_cast<int>(session->segments.size())) {
        return session->full_text.size();
    }
    return 0;
}

static size_t generic_text_stable_prefix_candidate_raw_bytes(transcribe_session * session) {
    if (session == nullptr || !session->has_result) {
        return 0;
    }
    // Library default when the caller leaves stable_prefix_agreement_n at
    // 0; documented as "currently 3" on transcribe_stream_params.
    static constexpr uint32_t k_default_stable_prefix_agreement_n = 3;
    uint32_t                  agreement_n                         = session->stream_stable_prefix_agreement_n;
    if (agreement_n == 0) {
        agreement_n = k_default_stable_prefix_agreement_n;
    }
    if (agreement_n <= 1) {
        return session->full_text.size();
    }

    session->stream_raw_history.push_back(session->full_text);
    while (session->stream_raw_history.size() > agreement_n) {
        session->stream_raw_history.pop_front();
    }
    if (session->stream_raw_history.size() < agreement_n) {
        return 0;
    }

    size_t prefix_n = session->stream_raw_history.front().size();
    for (const auto & text : session->stream_raw_history) {
        prefix_n = std::min(prefix_n, common_prefix_bytes(session->stream_raw_history.front(), text));
    }
    return utf8_floor_boundary(session->full_text, prefix_n);
}

static size_t selected_stable_prefix_candidate_raw_bytes(transcribe_session * session) {
    if (session == nullptr) {
        return 0;
    }
    const transcribe::Arch * arch = session->model != nullptr ? session->model->arch : nullptr;
    switch (stream_stable_prefix_impl_for_arch(arch)) {
        case StreamStablePrefixImpl::FamilyTokenAgreement:
        case StreamStablePrefixImpl::FamilyNativeCommit:
            return family_candidate_raw_prefix_bytes(session);
        case StreamStablePrefixImpl::GenericTextAgreement:
            return generic_text_stable_prefix_candidate_raw_bytes(session);
    }
    return 0;
}

static bool append_committed_raw_prefix(transcribe_session * session, size_t candidate_bytes) {
    if (session == nullptr || !session->has_result) {
        return false;
    }
    const std::string & raw          = session->full_text;
    const size_t        old_boundary = utf8_floor_boundary(
        raw, std::min<size_t>(static_cast<size_t>(session->stream_raw_tentative_start_bytes), raw.size()));
    const size_t candidate = utf8_floor_boundary(raw, std::min(candidate_bytes, raw.size()));
    if (candidate <= old_boundary) {
        return false;
    }
    if (old_boundary != session->stream_committed_text.size()) {
        return false;
    }
    if (raw.compare(0, old_boundary, session->stream_committed_text.data(), old_boundary) != 0) {
        return false;
    }
    session->stream_committed_text.append(raw.data() + old_boundary, candidate - old_boundary);
    session->stream_raw_tentative_start_bytes = static_cast<uint64_t>(candidate);
    return true;
}

static bool finalize_committed_text(transcribe_session * session) {
    if (session == nullptr || !session->has_result) {
        return false;
    }
    if (session->stream_commit_policy == TRANSCRIBE_STREAM_COMMIT_ON_FINALIZE ||
        session->stream_committed_text.empty()) {
        const bool changed                        = session->stream_committed_text != session->full_text;
        session->stream_committed_text            = session->full_text;
        session->stream_raw_tentative_start_bytes = static_cast<uint64_t>(session->full_text.size());
        return changed;
    }
    if (!starts_with_bytes(session->full_text, session->stream_committed_text)) {
        session->stream_raw_tentative_start_bytes = static_cast<uint64_t>(session->full_text.size());
        return false;
    }
    const size_t old_n = session->stream_committed_text.size();
    if (session->full_text.size() > old_n) {
        session->stream_committed_text.append(session->full_text.data() + old_n, session->full_text.size() - old_n);
        session->stream_raw_tentative_start_bytes = static_cast<uint64_t>(session->full_text.size());
        return true;
    }
    session->stream_raw_tentative_start_bytes = static_cast<uint64_t>(session->full_text.size());
    return false;
}

struct StreamTextDelta {
    bool committed_changed = false;
    bool tentative_changed = false;
};

static StreamTextDelta apply_stream_text_policy(transcribe_session * session, bool is_finalize) {
    StreamTextDelta delta;
    if (session == nullptr) {
        return delta;
    }
    const std::string prev_committed = session->stream_committed_text;
    const std::string prev_tentative = session->stream_tentative_text;

    if (is_finalize) {
        delta.committed_changed = finalize_committed_text(session);
        session->stream_tentative_text.clear();
    } else if (session->has_result) {
        size_t candidate = 0;
        switch (session->stream_commit_policy) {
            case TRANSCRIBE_STREAM_COMMIT_ON_FINALIZE:
                candidate = 0;
                break;
            case TRANSCRIBE_STREAM_COMMIT_STABLE_PREFIX:
            case TRANSCRIBE_STREAM_COMMIT_AUTO:
            default:
                candidate = selected_stable_prefix_candidate_raw_bytes(session);
                break;
        }
        delta.committed_changed = append_committed_raw_prefix(session, candidate);

        const size_t boundary = utf8_floor_boundary(
            session->full_text, std::min<size_t>(static_cast<size_t>(session->stream_raw_tentative_start_bytes),
                                                 session->full_text.size()));
        session->stream_tentative_text.assign(session->full_text.data() + boundary,
                                              session->full_text.size() - boundary);
    } else {
        session->stream_tentative_text.clear();
    }

    delta.committed_changed = delta.committed_changed || session->stream_committed_text != prev_committed;
    delta.tentative_changed = session->stream_tentative_text != prev_tentative;
    return delta;
}

static void publish_stream_update_tail(transcribe_stream_update * update,
                                       bool                       committed_changed,
                                       bool                       tentative_changed) {
    if (update == nullptr) {
        return;
    }
    if (has_field(update->struct_size, k_stream_update_committed_changed_size)) {
        update->committed_changed = committed_changed;
    }
    if (has_field(update->struct_size, k_stream_update_tentative_changed_size)) {
        update->tentative_changed = tentative_changed;
    }
}

// Compute the observable-change verdict, advance the revision counter
// when the result moved, and publish revision / result_changed /
// committed_changed / tentative_changed onto the caller's update. Shared
// verbatim by feed and finalize so the result_changed semantics live in
// exactly one place; the path-specific work (ON_FINALIZE counter zeroing
// and audio_committed_ms on feed, is_final and the lifecycle transition
// on finalize) stays in the callers.
static void publish_observable_delta(transcribe_session *       session,
                                     transcribe_stream_update * update,
                                     int32_t                    prev_revision,
                                     const std::string &        prev_full_text,
                                     bool                       prev_has_result,
                                     const StreamTextDelta &    text_delta) {
    const bool raw_changed        = session->has_result != prev_has_result || session->full_text != prev_full_text;
    const bool observable_changed = raw_changed || text_delta.committed_changed || text_delta.tentative_changed ||
                                    (update != nullptr && update->result_changed);
    if (observable_changed && session->stream_revision == prev_revision) {
        session->stream_revision += 1;
    }
    if (update != nullptr) {
        update->result_changed =
            update->result_changed || observable_changed || session->stream_revision != prev_revision;
        update->revision = session->stream_revision;
        publish_stream_update_tail(update, text_delta.committed_changed, text_delta.tentative_changed);
    }
}

// Advisory pnc/itn warning. Emits a WARN when a non-DEFAULT request hits a
// model that does not support runtime control of that axis, then returns so
// the dispatcher proceeds with best-effort semantics. The reserved
// TRANSCRIBE_ERR_UNSUPPORTED_PNC / _ITN codes are NOT returned today
// (placeholders for a future opt-in strict mode). The message includes the
// arch + variant strings to pinpoint which model dropped the request.
void warn_unsupported_advisory(const struct transcribe_model * model, const struct transcribe_run_params * rp) {
    if (model == nullptr || rp == nullptr) {
        return;
    }
    const char * arch_name = (model->arch != nullptr && model->arch->name != nullptr) ? model->arch->name : "(unknown)";
    const char * variant   = model->variant.c_str();
    if (variant == nullptr || variant[0] == '\0') {
        variant = "(unknown)";
    }

    char buf[512];
    if (rp->pnc != TRANSCRIBE_PNC_MODE_DEFAULT && !transcribe::has_feature(model, TRANSCRIBE_FEATURE_PNC)) {
        const char * req = (rp->pnc == TRANSCRIBE_PNC_MODE_ON) ? "ON" : "OFF";
        std::snprintf(buf, sizeof(buf),
                      "transcribe_run: caller requested pnc=%s but model '%s' "
                      "(variant '%s') does not support pnc control; output will use "
                      "the model's default behavior. Use "
                      "transcribe_model_supports(model, TRANSCRIBE_FEATURE_PNC) to "
                      "pre-check.",
                      req, arch_name, variant);
        transcribe_log_emit_or_stderr(TRANSCRIBE_LOG_LEVEL_WARN, buf);
    }
    if (rp->itn != TRANSCRIBE_ITN_MODE_DEFAULT && !transcribe::has_feature(model, TRANSCRIBE_FEATURE_ITN)) {
        const char * req = (rp->itn == TRANSCRIBE_ITN_MODE_ON) ? "ON" : "OFF";
        std::snprintf(buf, sizeof(buf),
                      "transcribe_run: caller requested itn=%s but model '%s' "
                      "(variant '%s') does not support itn control; output will use "
                      "the model's default behavior. Use "
                      "transcribe_model_supports(model, TRANSCRIBE_FEATURE_ITN) to "
                      "pre-check.",
                      req, arch_name, variant);
        transcribe_log_emit_or_stderr(TRANSCRIBE_LOG_LEVEL_WARN, buf);
    }
}

}  // namespace

// Lifecycle

static transcribe_status transcribe_model_load_file_impl(const char *                                path,
                                                         const struct transcribe_model_load_params * params,
                                                         struct transcribe_model **                  out_model) {
    // Set up ggml's timer before any family's load-timing ggml_time_us() runs
    // (Windows/MSVC ÷0 otherwise — see ensure_ggml_time_init).
    ensure_ggml_time_init();
    if (out_model == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    *out_model = nullptr;
    if (path == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    // NULL params means "all defaults". This is the version-proof
    // spelling of defaults — it carries no struct_size, so it always
    // matches the running library. A zeroed struct ({0}) is a different
    // thing: it claims struct_size == 0 and is rejected below.
    struct transcribe_model_load_params params_defaults;
    transcribe_model_load_params_init(&params_defaults);
    if (params == nullptr) {
        params = &params_defaults;
    }
    if (const auto st = check_input_struct_size(params->struct_size, k_min_model_params_size); st != TRANSCRIBE_OK) {
        return st;
    }

    // gpu_device is validated where the device registry is available — in
    // load_common::init_backends, which each family calls. 0 means auto/first
    // of kind; a positive index selects a specific GPU; negative / out of
    // range / kind-mismatched values return TRANSCRIBE_ERR_INVALID_ARG from
    // there. See the public header and transcribe-load-common.h.

    // Raw-validate the backend request before the families' first
    // enum-typed load of it (see enum_field_raw). init_backends re-checks
    // as defense in depth; this boundary check is what makes every
    // downstream `params->backend` read defined.
    switch (enum_field_raw(&params->backend)) {
        case TRANSCRIBE_BACKEND_AUTO:
        case TRANSCRIBE_BACKEND_CPU:
        case TRANSCRIBE_BACKEND_METAL:
        case TRANSCRIBE_BACKEND_VULKAN:
        case TRANSCRIBE_BACKEND_CPU_ACCEL:
        case TRANSCRIBE_BACKEND_CUDA:
            break;
        default:
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "transcribe_model_load_file: invalid backend request %d",
                                enum_field_raw(&params->backend));
            return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // Format sniff by reading the first four bytes (not the file extension,
    // which HF/users mislabel): GGUF magic 0x46554747 is the canonical path,
    // ggml magic 0x67676d6c is a legacy whisper.cpp .bin. Public contract:
    // missing path -> ERR_FILE_NOT_FOUND; every other failure -> ERR_GGUF.
    if (!transcribe::path_is_present(path)) {
        return TRANSCRIBE_ERR_FILE_NOT_FOUND;
    }
    uint32_t magic = 0;
    {
        std::ifstream fin(transcribe::path_from_utf8(path), std::ios::binary);
        if (!fin) {
            // The existence pre-check said the path is reachable but
            // the open failed — permissions / type issue. Treat as a generic
            // load failure rather than FILE_NOT_FOUND.
            return TRANSCRIBE_ERR_GGUF;
        }
        fin.read(reinterpret_cast<char *>(&magic), sizeof(magic));
        if (!fin || fin.gcount() != sizeof(magic)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "transcribe_model_load_file: short read on file "
                                "magic for %s",
                                path);
            return TRANSCRIBE_ERR_GGUF;
        }
    }

    // 0x67676d6c ("ggml" little-endian): legacy whisper.cpp .bin. Hand off
    // to the whisper .bin adapter, which validates the hparams as
    // whisper-shaped (rejecting unrelated ggml-magic files like Silero VAD).
    if (magic == 0x67676d6cu) {
        return transcribe::whisper::load_from_bin(path, params, out_model);
    }

    // Header-only GGUF inspection. The Loader is stack-allocated; if
    // anything below bails before transferring ownership, its destructor
    // frees the gguf_context on unwind.
    transcribe::Loader loader;
    if (const transcribe_status st = loader.open(path); st != TRANSCRIBE_OK) {
        return st;
    }

    // Per-family dispatch. The architecture string came from the GGUF KV so
    // the loader guarantees it is non-null and NUL-terminated.
    const transcribe::Arch * arch = transcribe::find_arch(loader.arch().c_str());
    if (arch == nullptr) {
        return TRANSCRIBE_ERR_UNSUPPORTED_ARCH;
    }

    // A registered family with no load entry point yet is treated as
    // not-implemented at the central dispatch level so a half-wired
    // handler does not crash.
    if (arch->load == nullptr) {
        return TRANSCRIBE_ERR_NOT_IMPLEMENTED;
    }

    // Hand off. The handler may take ownership of the gguf_context via
    // loader.release_gguf(); if it doesn't, the Loader destructor frees
    // it normally on stack unwinding.
    const transcribe_status st = arch->load(loader, params, out_model);

    // Copy the string-metadata map onto the model once, centrally. The
    // loader read it during open() and outlives arch->load(), so we set it
    // here instead of in every per-family handler. `variant` is owned by the
    // family (it may default it when stt.variant was absent) and stays on its
    // own accessor; this map is the generic general.* / display surface.
    if (st == TRANSCRIBE_OK && out_model != nullptr && *out_model != nullptr) {
        (*out_model)->meta = loader.meta();
    }
    return st;
}

static void transcribe_model_free_impl(struct transcribe_model * model) {
    // Polymorphic delete: the base has a virtual destructor (anchored
    // in transcribe-model.cpp), so this dispatches to the per-family
    // subclass destructor and that destructor frees gguf_context,
    // weights, etc. Passing NULL is a no-op per the public contract.
    delete model;
}

static transcribe_status transcribe_session_init_impl(struct transcribe_model *                model,
                                                      const struct transcribe_session_params * params,
                                                      struct transcribe_session **             out_session) {
    if (out_session == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    *out_session = nullptr;
    if (model == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    // NULL params means "all defaults" (see transcribe_model_load_file).
    struct transcribe_session_params params_defaults;
    transcribe_session_params_init(&params_defaults);
    if (params == nullptr) {
        params = &params_defaults;
    }
    if (const auto st = check_input_struct_size(params->struct_size, k_min_context_params_size); st != TRANSCRIBE_OK) {
        return st;
    }
    // n_threads contract from include/transcribe.h: 0 means "library
    // picks a sensible default", positive means "use this many." A
    // negative value is undefined input, not a documented sentinel —
    // reject it here so individual family handlers don't have to
    // re-check the same condition.
    if (params->n_threads < 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    // Raw-validate kv_type before the families' first enum-typed load of it
    // (see enum_field_raw): an unknown value from a C caller is a clean
    // error here, never UB downstream.
    switch (enum_field_raw(&params->kv_type)) {
        case TRANSCRIBE_KV_TYPE_AUTO:
        case TRANSCRIBE_KV_TYPE_F32:
        case TRANSCRIBE_KV_TYPE_F16:
            break;
        default:
            return TRANSCRIBE_ERR_INVALID_ARG;
    }
    // n_ctx is a trailing field (appended after kv_type), so only read it
    // when the caller's struct_size actually covers it; an older caller's
    // smaller struct leaves it at the default 0 = "model max". A negative
    // value is undefined input, not a documented sentinel. The family that
    // honors n_ctx clamps a too-large value down to the model maximum, so
    // only the negative case is rejected here. See include/transcribe.h.
    if (has_field(params->struct_size, offsetof(struct transcribe_session_params, n_ctx) + sizeof(params->n_ctx)) &&
        params->n_ctx < 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    // The model carries its own arch dispatch pointer. A model that
    // came from a load() call always has arch set; the null check is
    // defensive against a hypothetical malformed model object.
    if (model->arch == nullptr || model->arch->init_context == nullptr) {
        return TRANSCRIBE_ERR_NOT_IMPLEMENTED;
    }
    return model->arch->init_context(model, params, out_session);
}

static void transcribe_session_free_impl(struct transcribe_session * session) {
    // Free the session and, if it owns its model (transcribe_open path),
    // free the model too. The owns_model flag is set only by
    // transcribe_open; sessions created via the two-step
    // transcribe_session_init borrow the model and leave it alone here.
    //
    // Capture before the session is destroyed; reading session->model
    // after delete would be use-after-free.
    if (session == nullptr) {
        return;
    }
    struct transcribe_model * owned = session->owns_model ? session->model : nullptr;
    delete session;
    transcribe_model_free(owned);  // NULL is a no-op
}

// Convenience: open / close / get_model
//
// transcribe_open bundles load + session_init and hands back a session that
// owns its model (owns_model = true). transcribe_close is a thin alias for
// transcribe_session_free; both honor owns_model and free the owned model
// after the session.

static transcribe_status transcribe_open_impl(const char *                                path,
                                              const struct transcribe_model_load_params * load_params,
                                              const struct transcribe_session_params *    session_params,
                                              struct transcribe_session **                out_session) {
    if (out_session == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    *out_session = nullptr;

    // NULL params are forwarded as-is: the callees already treat NULL as
    // "all defaults", so no substitution is needed here.
    struct transcribe_model * model = nullptr;
    if (const auto st = transcribe_model_load_file(path, load_params, &model); st != TRANSCRIBE_OK) {
        return st;
    }

    struct transcribe_session * session = nullptr;
    if (const auto st = transcribe_session_init(model, session_params, &session); st != TRANSCRIBE_OK) {
        // Load succeeded but session init failed: own the cleanup so the
        // caller sees the same all-or-nothing contract as the two-step
        // API. No partial state leaks.
        transcribe_model_free(model);
        return st;
    }

    // The convenience session owns the model it loaded; either
    // transcribe_session_free or transcribe_close (its alias) will free
    // both. This is the only place owns_model is set.
    session->owns_model = true;
    *out_session        = session;
    return TRANSCRIBE_OK;
}

extern "C" void transcribe_close(struct transcribe_session * session) {
    // Alias for transcribe_session_free, kept for source compatibility
    // with the prior split-API shape. Both honor owns_model.
    transcribe_session_free(session);
}

extern "C" const struct transcribe_model * transcribe_get_model(const struct transcribe_session * session) {
    if (session == nullptr) {
        return nullptr;
    }
    return session->model;
}

extern "C" void transcribe_set_abort_callback(struct transcribe_session * session,
                                              transcribe_abort_callback   cb,
                                              void *                      user_data) {
    if (session == nullptr) {
        return;
    }
    session->abort_cb       = cb;
    session->abort_userdata = user_data;
}

extern "C" bool transcribe_was_aborted(const struct transcribe_session * session) {
    if (session == nullptr) {
        return false;
    }
    return session->was_aborted;
}

extern "C" bool transcribe_was_truncated(const struct transcribe_session * session) {
    if (session == nullptr) {
        return false;
    }
    return session->was_truncated;
}

// Streaming dispatcher
//
// State transitions are managed entirely here; hooks see ACTIVE on entry to
// begin/feed/finalize and never observe transitions. Hooks own the
// per-utterance result data (tokens/words/segments, committed counts, audio
// cursors, stream_revision); the dispatcher owns lifecycle state,
// last_status, and the public committed/tentative text view. clear_result()
// wipes the snapshot but never touches stream_state.

static transcribe_status transcribe_stream_begin_impl(struct transcribe_session *             session,
                                                      const struct transcribe_run_params *    run_params,
                                                      const struct transcribe_stream_params * stream_params) {
    if (session == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    // NULL run/stream params each mean "all defaults".
    struct transcribe_run_params run_params_defaults;
    transcribe_run_params_init(&run_params_defaults);
    struct transcribe_stream_params stream_params_defaults;
    transcribe_stream_params_init(&stream_params_defaults);
    if (run_params == nullptr) {
        run_params = &run_params_defaults;
    }
    if (stream_params == nullptr) {
        stream_params = &stream_params_defaults;
    }
    if (const auto st = check_input_struct_size(run_params->struct_size, k_min_run_params_size); st != TRANSCRIBE_OK) {
        return st;
    }
    if (const auto st = check_input_struct_size(stream_params->struct_size, k_min_stream_params_size);
        st != TRANSCRIBE_OK) {
        return st;
    }
    // Read the caller's commit_policy as raw bytes and validate BEFORE the
    // first load through the enum-typed lvalue: C callers can legally store
    // any int there, and an out-of-range enum load is UB in C++.
    int      commit_policy_raw         = TRANSCRIBE_STREAM_COMMIT_AUTO;
    uint32_t stable_prefix_agreement_n = 0;
    if (has_field(stream_params->struct_size, k_stream_params_commit_policy_size)) {
        static_assert(sizeof(stream_params->commit_policy) == sizeof(int),
                      "commit_policy must be int-sized for the raw read");
        std::memcpy(&commit_policy_raw, &stream_params->commit_policy, sizeof(commit_policy_raw));
    }
    if (!valid_stream_commit_policy(commit_policy_raw)) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    const transcribe_stream_commit_policy commit_policy =
        static_cast<transcribe_stream_commit_policy>(commit_policy_raw);
    if (has_field(stream_params->struct_size, k_stream_params_agreement_n_size)) {
        stable_prefix_agreement_n = stream_params->stable_prefix_agreement_n;
        if (stable_prefix_agreement_n > 32) {
            return TRANSCRIBE_ERR_INVALID_ARG;
        }
    }
    if (session->stream_state == TRANSCRIBE_STREAM_ACTIVE) {
        // Caller must finalize or reset before starting a new stream.
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (session->model == nullptr || session->model->arch == nullptr) {
        return TRANSCRIBE_ERR_NOT_IMPLEMENTED;
    }
    // Capability gate: the model must advertise streaming AND the
    // family must wire the full required hook set. begin / feed /
    // finalize come as a triple — a partially-wired family that
    // accepts begin would otherwise let the caller enter ACTIVE and
    // then get stuck on NOT_IMPLEMENTED at the first feed. stream_reset
    // remains optional; the dispatcher's clear_result + state wipe is
    // sufficient when a family does not need to release per-utterance
    // buffers explicitly.
    if (!session->model->caps.supports_streaming || session->model->arch->stream_begin == nullptr ||
        session->model->arch->stream_feed == nullptr || session->model->arch->stream_finalize == nullptr) {
        return TRANSCRIBE_ERR_NOT_IMPLEMENTED;
    }

    if (const transcribe_status st = validate_run_params_common(session, run_params); st != TRANSCRIBE_OK) {
        return st;
    }

    // v1 rejects TRANSLATE unconditionally for streaming. A future
    // family that supports streaming translate would loosen this in
    // its stream_begin hook, but the central dispatcher refuses
    // upfront so partially-wired callers fail fast.
    if (run_params->task == TRANSCRIBE_TASK_TRANSLATE) {
        return TRANSCRIBE_ERR_UNSUPPORTED_TASK;
    }

    if (stream_params->family != nullptr) {
        if (stream_params->family->size < sizeof(struct transcribe_ext)) {
            return TRANSCRIBE_ERR_BAD_STRUCT_SIZE;
        }
        if (!transcribe_model_accepts_ext_kind(session->model, TRANSCRIBE_EXT_SLOT_STREAM,
                                               stream_params->family->kind)) {
            return TRANSCRIBE_ERR_INVALID_ARG;
        }
    }

    // Advisory warn for pnc/itn requests against models that don't
    // expose the corresponding runtime toggle. Emitted before
    // clear_result so the pre-hook "snapshot preserved on rejection"
    // contract is undisturbed.
    warn_unsupported_advisory(session->model, run_params);

    // Optional family preflight: validates extension field values
    // (e.g. parakeet's (L, C, R) menu) without mutating state. On
    // non-OK return the previous snapshot and lifecycle are preserved
    // so a caller-side typo does not destroy the prior utterance.
    if (session->model->arch->stream_validate != nullptr) {
        if (const transcribe_status st = session->model->arch->stream_validate(session, run_params, stream_params);
            st != TRANSCRIBE_OK) {
            return st;
        }
    }

    // All checks pass — clear the previous snapshot and hand off.
    session->clear_result();
    session->t_mel_us                         = 0;
    session->t_encode_us                      = 0;
    session->t_decode_us                      = 0;
    session->was_aborted                      = false;
    session->was_truncated                    = false;
    session->stream_state                     = TRANSCRIBE_STREAM_ACTIVE;
    session->stream_commit_policy             = commit_policy;
    session->stream_stable_prefix_agreement_n = stable_prefix_agreement_n;

    // Hand the family hook a params view whose pointers the LIBRARY owns.
    // Families may capture `*run_params` for the stream's lifetime
    // (parakeet re-reads .language on every feed), and the public contract
    // lets the caller free every params pointer once begin returns — so the
    // strings are copied into session storage here and the view repointed
    // at it. `family` (the run-slot extension) is nulled in the view: no
    // family reads it on the stream path today, and per the ext copy-out
    // contract a retained pointer to it would dangle; a future family that
    // wants a run-slot ext at stream begin must plumb it deliberately.
    session->stream_language_owned        = run_params->language != nullptr ? run_params->language : "";
    session->stream_target_language_owned = run_params->target_language != nullptr ? run_params->target_language : "";
    // PREFIX copy, not struct assignment: the size gate above admits any
    // struct_size >= k_min_run_params_size, so a conforming caller's
    // allocation may be SHORTER than sizeof (fields past `family`, e.g.
    // spec_k_drafts, absent). Init first so bytes past the caller's
    // prefix hold their documented defaults, then copy only what the
    // caller owns. The caller's struct_size is preserved by the copy, so
    // downstream has_field() gating still sees the caller's true layout.
    struct transcribe_run_params run_params_owned;
    transcribe_run_params_init(&run_params_owned);
    std::memcpy(&run_params_owned, run_params,
                static_cast<size_t>(std::min<uint64_t>(run_params->struct_size, sizeof(run_params_owned))));
    run_params_owned.language = run_params->language != nullptr ? session->stream_language_owned.c_str() : nullptr;
    run_params_owned.target_language =
        run_params->target_language != nullptr ? session->stream_target_language_owned.c_str() : nullptr;
    run_params_owned.family = nullptr;

    const transcribe_status st = session->model->arch->stream_begin(session, &run_params_owned, stream_params);
    if (st != TRANSCRIBE_OK) {
        // Family hook rejected the begin (config it does not understand,
        // memory allocation failure, etc.). Roll lifecycle back to
        // FAILED so the caller can read last_status; leave the cleared
        // result snapshot in place.
        session->stream_state       = TRANSCRIBE_STREAM_FAILED;
        session->stream_last_status = st;
    }
    return st;
}

// Helper: validate the caller's stream_update struct_size and reset
// every field except struct_size to a clean state before handing the
// update buffer to the family hook. Returns OK when update is NULL
// (the update buffer is optional) or when the size passes validation.
static transcribe_status prepare_stream_update(struct transcribe_stream_update * update) {
    if (update == nullptr) {
        return TRANSCRIBE_OK;
    }
    if (const auto st = check_struct_size(update->struct_size, k_min_stream_update_size); st != TRANSCRIBE_OK) {
        return st;
    }
    const uint64_t           caller_size = update->struct_size;
    transcribe_stream_update staged{};
    staged.struct_size = caller_size;
    copy_out_prefix(update, &staged, caller_size, sizeof(staged));
    return TRANSCRIBE_OK;
}

static transcribe_status transcribe_stream_feed_impl(struct transcribe_session *       session,
                                                     const float *                     pcm,
                                                     int                               n_samples,
                                                     struct transcribe_stream_update * update) {
    if (const auto st = prepare_stream_update(update); st != TRANSCRIBE_OK) {
        return st;
    }
    if (session == nullptr || pcm == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    // Zero-length feed is rejected: feed exists to consume audio,
    // and callers that just want to inspect state use the stream
    // accessors. n_samples < 0 is plain garbage.
    if (n_samples <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (session->stream_state != TRANSCRIBE_STREAM_ACTIVE) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    // begin already confirmed model/arch/hook; we re-check defensively
    // so a malformed context (never happens in practice from a real
    // begin) does not deref a null function pointer.
    if (session->model == nullptr || session->model->arch == nullptr || session->model->arch->stream_feed == nullptr) {
        return TRANSCRIBE_ERR_NOT_IMPLEMENTED;
    }

    const int32_t     prev_revision   = session->stream_revision;
    const std::string prev_full_text  = session->full_text;
    const bool        prev_has_result = session->has_result;

    const transcribe_status st = session->model->arch->stream_feed(session, pcm, n_samples, update);
    if (st != TRANSCRIBE_OK) {
        session->stream_state       = TRANSCRIBE_STREAM_FAILED;
        session->stream_last_status = st;
        return st;
    }
    const StreamTextDelta text_delta = apply_stream_text_policy(session, /*is_finalize=*/false);
    if (session->stream_commit_policy == TRANSCRIBE_STREAM_COMMIT_ON_FINALIZE) {
        session->n_committed_tokens   = 0;
        session->n_committed_words    = 0;
        session->n_committed_segments = 0;
    }
    publish_observable_delta(session, update, prev_revision, prev_full_text, prev_has_result, text_delta);
    if (update != nullptr && session->stream_commit_policy == TRANSCRIBE_STREAM_COMMIT_ON_FINALIZE) {
        update->audio_committed_ms = 0;
    }
    return st;
}

static transcribe_status transcribe_stream_finalize_impl(struct transcribe_session *       session,
                                                         struct transcribe_stream_update * update) {
    if (const auto st = prepare_stream_update(update); st != TRANSCRIBE_OK) {
        return st;
    }
    if (session == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (session->stream_state != TRANSCRIBE_STREAM_ACTIVE) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (session->model == nullptr || session->model->arch == nullptr ||
        session->model->arch->stream_finalize == nullptr) {
        return TRANSCRIBE_ERR_NOT_IMPLEMENTED;
    }

    const int32_t     prev_revision   = session->stream_revision;
    const std::string prev_full_text  = session->full_text;
    const bool        prev_has_result = session->has_result;

    const transcribe_status st = session->model->arch->stream_finalize(session, update);
    if (update != nullptr) {
        // Force is_final true regardless of family hook return; the
        // marker describes the call site, not the result. Set after
        // the hook so the family cannot accidentally clear it.
        update->is_final = true;
    }
    if (st != TRANSCRIBE_OK) {
        session->stream_state       = TRANSCRIBE_STREAM_FAILED;
        session->stream_last_status = st;
        return st;
    }
    const StreamTextDelta text_delta = apply_stream_text_policy(session, /*is_finalize=*/true);
    publish_observable_delta(session, update, prev_revision, prev_full_text, prev_has_result, text_delta);
    session->stream_state = TRANSCRIBE_STREAM_FINISHED;
    return TRANSCRIBE_OK;
}

static void transcribe_stream_reset_impl(struct transcribe_session * session) {
    if (session == nullptr) {
        return;
    }
    // Family hook releases per-utterance state and clears any buffered
    // audio contents while keeping the allocations. A family without
    // streaming just has no hook installed; reset becomes a pure
    // dispatcher state wipe.
    if (session->model != nullptr && session->model->arch != nullptr && session->model->arch->stream_reset != nullptr) {
        session->model->arch->stream_reset(session);
    }
    session->clear_result();
    session->stream_state = TRANSCRIBE_STREAM_IDLE;
    // was_aborted is per-stream; reset re-arms it the same way begin
    // does so a caller that resets after an abort starts clean.
    session->was_aborted  = false;
}

extern "C" enum transcribe_stream_state transcribe_stream_get_state(const struct transcribe_session * session) {
    if (session == nullptr) {
        return TRANSCRIBE_STREAM_IDLE;
    }
    return session->stream_state;
}

extern "C" int transcribe_stream_revision(const struct transcribe_session * session) {
    if (session == nullptr) {
        return 0;
    }
    return session->stream_revision;
}

extern "C" int transcribe_stream_n_committed_segments(const struct transcribe_session * session) {
    if (session == nullptr) {
        return 0;
    }
    return session->n_committed_segments;
}

extern "C" int transcribe_stream_n_committed_words(const struct transcribe_session * session) {
    if (session == nullptr) {
        return 0;
    }
    return session->n_committed_words;
}

extern "C" int transcribe_stream_n_committed_tokens(const struct transcribe_session * session) {
    if (session == nullptr) {
        return 0;
    }
    return session->n_committed_tokens;
}

extern "C" transcribe_status transcribe_stream_last_status(const struct transcribe_session * session) {
    if (session == nullptr) {
        return TRANSCRIBE_OK;
    }
    return session->stream_last_status;
}

extern "C" transcribe_status transcribe_stream_get_text(const struct transcribe_session * session,
                                                        struct transcribe_stream_text *   out) {
    if (out == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (const auto st = check_struct_size(out->struct_size, k_min_stream_text_size); st != TRANSCRIBE_OK) {
        return st;
    }
    const uint64_t         caller_size = out->struct_size;
    transcribe_stream_text staged{};
    staged.struct_size = caller_size;
    if (session != nullptr && session->has_result) {
        staged.full_text            = session->full_text.c_str();
        staged.full_text_bytes      = static_cast<uint64_t>(session->full_text.size());
        staged.committed_text       = session->stream_committed_text.c_str();
        staged.committed_text_bytes = static_cast<uint64_t>(session->stream_committed_text.size());
        staged.tentative_text       = session->stream_tentative_text.c_str();
        staged.tentative_text_bytes = static_cast<uint64_t>(session->stream_tentative_text.size());
        staged.raw_tentative_start_bytes =
            std::min<uint64_t>(session->stream_raw_tentative_start_bytes, staged.full_text_bytes);
    } else {
        staged.full_text      = "";
        staged.committed_text = "";
        staged.tentative_text = "";
    }
    copy_out_prefix(out, &staged, caller_size, sizeof(staged));
    return TRANSCRIBE_OK;
}

// Shared one-utterance run body. Does NOT touch session->batch_results, so
// the batch dispatcher can call it once per utterance inside a loop without
// erasing already-accumulated entries; the public transcribe_run wrapper
// below clears batch_results once before delegating here. Every early
// return preserves the previous result snapshot exactly as the original
// transcribe_run contract documented (see the inline comments).
static transcribe_status run_one_inner(struct transcribe_session *          session,
                                       const float *                        pcm,
                                       int                                  n_samples,
                                       const struct transcribe_run_params * params) {
    // Parameter-shape validation runs first and does not touch session
    // state. A caller that passes NULL pointers or a non-positive sample
    // count gets ERR_INVALID_ARG back without any visible side effect
    // — including the session's result fields, which are preserved
    // across a malformed call. This is the narrower half of the
    // "transcribe_run replaces the previous result" contract: the
    // replacement happens when the call is well-formed enough that
    // the dispatcher is willing to dereference session. A call that
    // isn't well-formed isn't considered to have "run".
    if (session == nullptr || pcm == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    // n_samples must be strictly positive, matching transcribe_stream_feed.
    // There is no meaningful "transcribe zero samples" operation; a
    // zero-length batch is treated as a caller error, not an empty run.
    if (n_samples <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    // NULL params means "all defaults" (transcribe vs translate, no
    // timestamps, etc.). A well-formed default run is not a malformed
    // call, so it proceeds and replaces the previous result.
    struct transcribe_run_params params_defaults;
    transcribe_run_params_init(&params_defaults);
    if (params == nullptr) {
        params = &params_defaults;
    }
    if (const auto st = check_input_struct_size(params->struct_size, k_min_run_params_size); st != TRANSCRIBE_OK) {
        return st;
    }
    // A run cannot replace an active stream's results — that would
    // strand the in-flight stream's per-family state. Caller must
    // finalize or reset first. FINISHED and FAILED both fall through;
    // the run path below resets stream_state to IDLE.
    if (session->stream_state == TRANSCRIBE_STREAM_ACTIVE) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // Pre-clear family-extension SHAPE validation: a run ext with a bad
    // transcribe_ext header size or a kind the model does not accept must
    // NOT wipe the previous result snapshot (mirrors the
    // transcribe_stream_begin contract). When model is null we skip the
    // pre-clear check and let the post-clear NOT_IMPLEMENTED path handle it.
    //
    // This guarantee covers ext shape (size/kind) and the run-param checks
    // below. A family that defers deeper value validation to run() (whisper
    // does this for prompt semantics) can still reject a correctly-shaped
    // ext after the snapshot is cleared; full pre-clear safety requires
    // validating values in run_validate.
    if (params->family != nullptr && session->model != nullptr && session->model->arch != nullptr) {
        if (params->family->size < sizeof(struct transcribe_ext)) {
            return TRANSCRIBE_ERR_BAD_STRUCT_SIZE;
        }
        if (!transcribe_model_accepts_ext_kind(session->model, TRANSCRIBE_EXT_SLOT_RUN, params->family->kind)) {
            return TRANSCRIBE_ERR_INVALID_ARG;
        }
    }

    // Advisory warn for pnc/itn requests against models that don't
    // expose the corresponding runtime toggle. Best-effort semantics:
    // log a WARN and proceed with the model's default behavior. Emitted
    // before clear_result so a malformed advisory doesn't disturb the
    // previous snapshot. Skipped when model is null — the post-clear
    // NOT_IMPLEMENTED path will signal that more directly.
    if (session->model != nullptr) {
        warn_unsupported_advisory(session->model, params);

        // Pre-clear run-param validation. A caller-side param bug
        // (out-of-range enum, timestamp granularity finer than the
        // model's max, unsupported language, or TRANSLATE against a
        // model that doesn't support it) must NOT wipe the previous
        // result snapshot — the same contract transcribe_stream_begin
        // honors for its run params. These checks read model->caps and
        // are therefore guarded by model != null; the degenerate
        // model-null case falls through to the post-clear
        // NOT_IMPLEMENTED path, which keeps its existing snapshot-wipe
        // behavior.
        if (const transcribe_status st = validate_run_params_common(session, params); st != TRANSCRIBE_OK) {
            return st;
        }
        if (params->task == TRANSCRIBE_TASK_TRANSLATE && !session->model->caps.supports_translate) {
            return TRANSCRIBE_ERR_UNSUPPORTED_TASK;
        }

        // Family run-ext validation (the _RUN analogue of stream_validate),
        // the final pre-clear gate. Runs AFTER the run-param checks above,
        // mirroring transcribe_stream_begin's ordering (dispatcher param
        // checks, then the family validate hook, then clear). The family
        // alone knows its per-kind minimum struct size; whisper enforces
        // that here. A non-OK return preserves the previous snapshot. A
        // family with no _RUN ext leaves this hook NULL. Called regardless
        // of whether params->family is set (whisper's hook treats a NULL
        // family as "defaults", returning OK).
        if (session->model->arch != nullptr && session->model->arch->run_validate != nullptr) {
            if (const transcribe_status st = session->model->arch->run_validate(session, params); st != TRANSCRIBE_OK) {
                return st;
            }
        }
    }

    // Result-replacement contract: everything past this point either
    // succeeds and writes a fresh result, or fails and leaves the
    // context in the documented "no result" sentinel state. Clear
    // eagerly so the one remaining downstream rejection path —
    // NOT_IMPLEMENTED on an incomplete arch — inherits the sentinel
    // without having to remember to call clear_result() itself.
    // Caller-param rejections (enum range, timestamp ceiling,
    // language, TRANSLATE support) are validated ABOVE, before this
    // clear, so a malformed call preserves the previous snapshot
    // exactly as transcribe_stream_begin does.
    //
    // Per-family run() handlers call clear_result() again after
    // their own front-matter checks succeed; that call is now
    // redundant but idempotent, and removing it is a refactor
    // deferred to a later pass.
    session->clear_result();
    session->t_mel_us      = 0;
    session->t_encode_us   = 0;
    session->t_decode_us   = 0;
    session->was_aborted   = false;
    session->was_truncated = false;
    // Force stream_state to IDLE: clear_result deliberately preserves
    // lifecycle state, but a well-formed transcribe_run subsumes any
    // prior FINISHED/FAILED stream — after a one-shot run the context
    // is no longer meaningfully in a streaming lifecycle.
    session->stream_state  = TRANSCRIBE_STREAM_IDLE;

    if (session->model == nullptr || session->model->arch == nullptr || session->model->arch->run == nullptr) {
        return TRANSCRIBE_ERR_NOT_IMPLEMENTED;
    }

    return session->model->arch->run(session, pcm, n_samples, params);
}

static transcribe_status transcribe_run_impl(struct transcribe_session *          session,
                                             const float *                        pcm,
                                             int                                  n_samples,
                                             const struct transcribe_run_params * params) {
    // A well-formed single run has the same result view as a one-item batch:
    // reset batch_results so the transcribe_batch_* accessors fall back to
    // reading the scratch slot as utterance 0.
    //
    // Guard the deref with the SAME pre-deref conditions run_one_inner uses
    // (non-null session + non-null pcm + positive n_samples). A malformed
    // call must not touch the session at all — the API smoke test probes
    // this with a fake (session *)0x1 and a NULL pcm / non-positive
    // n_samples, expecting INVALID_ARG with no dereference.
    if (session != nullptr && pcm != nullptr && n_samples > 0) {
        session->batch_results.clear();
    }
    return run_one_inner(session, pcm, n_samples, params);
}

// Batch run (offline)
//
// transcribe_run_batch validates the shared run_params ONCE, then either
// delegates to the family's batched run_batch() fast path or falls back to
// run_one_inner() per utterance. Either way the result is N entries in
// session->batch_results read back via the transcribe_batch_* accessors.

namespace {

// Copy the session's scratch result slot into a ResultSet snapshot.
transcribe_session::ResultSet snapshot_scratch_result(const transcribe_session * s, transcribe_status status) {
    transcribe_session::ResultSet rs;
    rs.tokens            = s->tokens;
    rs.words             = s->words;
    rs.segments          = s->segments;
    rs.full_text         = s->full_text;
    rs.detected_language = s->detected_language;
    rs.result_kind       = s->result_kind;
    rs.has_result        = s->has_result;
    rs.status            = status;
    return rs;
}

// Pad batch_results out to `n` entries with explicit aborted failures.
// Called only on an aborted batch: synthesized slots carry
// TRANSCRIBE_ERR_ABORTED ("did not complete because the batch was aborted"),
// so the result-set view has one slot per input utterance
// (transcribe_batch_n_results() == n) regardless of family or path.
void pad_batch_results_aborted(transcribe_session * s, int n) {
    while (s->batch_results.size() < static_cast<size_t>(n)) {
        transcribe_session::ResultSet rs;
        rs.status = TRANSCRIBE_ERR_ABORTED;
        s->batch_results.push_back(std::move(rs));
    }
}

// Restore the scratch slot from a ResultSet so the single-result accessors
// stay coherent after a batch run (they reflect utterance 0).
void restore_scratch_from_result(transcribe_session * s, const transcribe_session::ResultSet & rs) {
    s->tokens            = rs.tokens;
    s->words             = rs.words;
    s->segments          = rs.segments;
    s->full_text         = rs.full_text;
    s->detected_language = rs.detected_language;
    s->result_kind       = rs.result_kind;
    s->has_result        = rs.has_result;
}

}  // namespace

static transcribe_status transcribe_run_batch_impl(struct transcribe_session *          session,
                                                   const float * const *                pcm,
                                                   const int *                          n_samples,
                                                   int                                  n,
                                                   const struct transcribe_run_params * params) {
    // Top-level argument shape. The arrays themselves must be present and
    // n strictly positive; an individual malformed utterance (pcm[i] NULL
    // or n_samples[i] <= 0) is a per-utterance failure handled in the loop,
    // not a whole-batch error. As with transcribe_run, these checks do not
    // mutate the session, so a malformed call preserves the prior result.
    if (session == nullptr || pcm == nullptr || n_samples == nullptr || n <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    struct transcribe_run_params params_defaults;
    transcribe_run_params_init(&params_defaults);
    if (params == nullptr) {
        params = &params_defaults;
    }
    if (const auto st = check_input_struct_size(params->struct_size, k_min_run_params_size); st != TRANSCRIBE_OK) {
        return st;
    }
    if (session->stream_state == TRANSCRIBE_STREAM_ACTIVE) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // Shared-param validation, ONCE, mirroring transcribe_run's pre-clear
    // gates (ext shape/kind, pnc/itn advisory, enum range, timestamp
    // ceiling, language, TRANSLATE support, run_validate). A rejection here
    // preserves the previous result snapshot — nothing has been cleared.
    if (params->family != nullptr && session->model != nullptr && session->model->arch != nullptr) {
        if (params->family->size < sizeof(struct transcribe_ext)) {
            return TRANSCRIBE_ERR_BAD_STRUCT_SIZE;
        }
        if (!transcribe_model_accepts_ext_kind(session->model, TRANSCRIBE_EXT_SLOT_RUN, params->family->kind)) {
            return TRANSCRIBE_ERR_INVALID_ARG;
        }
    }
    if (session->model != nullptr) {
        warn_unsupported_advisory(session->model, params);
        if (const transcribe_status st = validate_run_params_common(session, params); st != TRANSCRIBE_OK) {
            return st;
        }
        if (params->task == TRANSCRIBE_TASK_TRANSLATE && !session->model->caps.supports_translate) {
            return TRANSCRIBE_ERR_UNSUPPORTED_TASK;
        }
        if (session->model->arch != nullptr && session->model->arch->run_validate != nullptr) {
            if (const transcribe_status st = session->model->arch->run_validate(session, params); st != TRANSCRIBE_OK) {
                return st;
            }
        }
    }

    if (session->model == nullptr || session->model->arch == nullptr || session->model->arch->run == nullptr) {
        return TRANSCRIBE_ERR_NOT_IMPLEMENTED;
    }

    // Past this point we commit to producing a fresh batch result.
    session->clear_result();
    session->t_mel_us      = 0;
    session->t_encode_us   = 0;
    session->t_decode_us   = 0;
    session->was_aborted   = false;
    session->was_truncated = false;
    session->stream_state  = TRANSCRIBE_STREAM_IDLE;
    session->batch_results.clear();

    // Fast path: a family with a batched compute graph owns the whole loop.
    if (session->model->arch->run_batch != nullptr) {
        const transcribe_status st = session->model->arch->run_batch(session, pcm, n_samples, n, params);
        // On abort the hook may retain only completed results; synthesize any
        // missing slots so the result-set view always exposes n entries.
        if (st == TRANSCRIBE_ERR_ABORTED) {
            pad_batch_results_aborted(session, n);
        }
        // Keep the single accessors coherent with utterance 0.
        if (!session->batch_results.empty()) {
            restore_scratch_from_result(session, session->batch_results.front());
        }
        return st;
    }

    // Generic serial fallback: run each utterance in turn and snapshot it.
    // Correct for every family; only the per-dispatch device throughput of
    // a real run_batch() is forgone.
    session->batch_results.reserve(static_cast<size_t>(n));
    transcribe_status batch_status = TRANSCRIBE_OK;
    for (int i = 0; i < n; ++i) {
        if (session->poll_abort()) {
            batch_status = TRANSCRIBE_ERR_ABORTED;
            break;
        }
        // run_one_inner clears the scratch slot and writes this utterance's
        // result; it re-validates the shared params (idempotent) and
        // validates this utterance's pcm[i] / n_samples[i].
        const transcribe_status st = run_one_inner(session, pcm[i], n_samples[i], params);
        if (st == TRANSCRIBE_OK) {
            session->batch_results.push_back(snapshot_scratch_result(session, st));
        } else {
            // Malformed-input early returns preserve the previous scratch
            // slot, so do NOT snapshot it — record an explicit empty
            // failure for this utterance instead.
            transcribe_session::ResultSet rs;
            rs.status = st;
            session->batch_results.push_back(std::move(rs));
            if (st == TRANSCRIBE_ERR_ABORTED) {
                batch_status = TRANSCRIBE_ERR_ABORTED;
                break;
            }
        }
    }

    // On abort the loop can break early, leaving fewer than n entries;
    // synthesize any missing slots so the result-set view always exposes n
    // entries (same invariant the fast path holds).
    if (batch_status == TRANSCRIBE_ERR_ABORTED) {
        pad_batch_results_aborted(session, n);
    }

    // Restore the scratch slot to mirror utterance 0 for the single-result
    // accessors (n >= 1 guarantees at least one entry, but be defensive).
    if (!session->batch_results.empty()) {
        restore_scratch_from_result(session, session->batch_results.front());
    }
    return batch_status;
}

// Model introspection
//
// These accessors read directly from the base struct; per-family load()
// fills caps / variant / backend before returning success.

extern "C" transcribe_status transcribe_model_get_capabilities(const struct transcribe_model *  model,
                                                               struct transcribe_capabilities * out_caps) {
    if (model == nullptr || out_caps == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (const auto st = check_struct_size(out_caps->struct_size, k_min_capabilities_size); st != TRANSCRIBE_OK) {
        return st;
    }
    // Preserve the caller-declared size, then write only the prefix
    // that fits in both the caller's buffer and this library's view;
    // any tail bytes stay as the caller initialized them (zero, by
    // the init function's zero-fill contract).
    //
    // Pointer fields written into the caller buffer (e.g. languages)
    // remain model-owned and valid until transcribe_model_free().
    const uint64_t          caller_size = out_caps->struct_size;
    transcribe_capabilities staged      = model->caps;
    staged.struct_size                  = caller_size;
    copy_out_prefix(out_caps, &staged, caller_size, sizeof(staged));
    return TRANSCRIBE_OK;
}

extern "C" transcribe_status transcribe_session_get_limits(const struct transcribe_session *  session,
                                                           struct transcribe_session_limits * out) {
    if (session == nullptr || out == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (const auto st = check_struct_size(out->struct_size, k_min_session_limits_size); st != TRANSCRIBE_OK) {
        return st;
    }

    transcribe_session_limits staged;
    transcribe_session_limits_init(&staged);

    const transcribe_model * model = session->model;
    const auto &             lb    = model->limits;

    if (lb.has_context_cap && lb.model_max_ctx > 0) {
        // effective_n_ctx = model max, lowered (never raised) by the session
        // n_ctx cap. This mirrors the per-family context_ceiling helpers.
        int32_t eff = lb.model_max_ctx;
        if (session->n_ctx > 0 && session->n_ctx < eff) {
            eff = session->n_ctx;
        }
        staged.effective_n_ctx = eff;

        // effective_max_audio_ms: for families whose audio tokens consume the
        // decoder context, invert the input gate at the effective ceiling so
        // the audio bound tracks n_ctx. For families whose audio bound is the
        // encoder (audio_from_caps), the audio limit is independent of the
        // decoder context, so report caps.max_audio_ms unchanged. Either way
        // it is advisory (representative prompt), not an exact per-call bound.
        if (lb.audio_from_caps) {
            staged.effective_max_audio_ms = model->caps.max_audio_ms;
        } else {
            const int64_t audio_tokens    = (int64_t) eff - lb.prompt_overhead - lb.gen_reserve;
            staged.effective_max_audio_ms = (audio_tokens > 0 && lb.ms_per_audio_token > 0.0) ?
                                                (int64_t) ((double) audio_tokens * lb.ms_per_audio_token) :
                                                0;
        }

        // max_kv_bytes: worst-case single-utterance KV allocation at the
        // effective ceiling, exact for the session's kv_type. The families
        // resolve AUTO (and F16) to f16 for the KV cache and use f32 only for
        // an explicit F32 request, so the byte size is 4/elem for F32 and
        // 2/elem otherwise. This is the ceiling for one utterance, not the
        // per-run allocation (the cache grows to fit input); transcribe_run_batch
        // allocates roughly batch_size x this.
        const int64_t kv_bytes_per_elem = (session->kv_type == TRANSCRIBE_KV_TYPE_F32) ? 4 : 2;
        staged.max_kv_bytes             = lb.kv_elems_per_ctx_token * (int64_t) eff * kv_bytes_per_elem;
    } else {
        // Unbounded / soft-window family: no decoder context cap. Report the
        // soft window (if any) from caps.max_audio_ms, independent of n_ctx.
        staged.effective_n_ctx        = 0;
        staged.effective_max_audio_ms = model->caps.max_audio_ms;
        staged.max_kv_bytes           = 0;
    }

    const uint64_t caller_size = out->struct_size;
    staged.struct_size         = caller_size;
    copy_out_prefix(out, &staged, caller_size, sizeof(staged));
    return TRANSCRIBE_OK;
}

extern "C" const char * transcribe_model_arch_string(const struct transcribe_model * model) {
    // The dispatch token's name field has static lifetime and is the
    // canonical arch string; we never store it again on the model.
    if (model == nullptr || model->arch == nullptr) {
        return "";
    }
    return model->arch->name != nullptr ? model->arch->name : "";
}

extern "C" const char * transcribe_model_variant_string(const struct transcribe_model * model) {
    if (model == nullptr) {
        return "";
    }
    return model->variant.c_str();
}

extern "C" const char * transcribe_model_meta_val_str(const struct transcribe_model * model, const char * key) {
    // Generic GGUF string-metadata getter, mirroring llama_model_meta_val_str.
    // Returns a model-owned string (valid until the model is freed) or "" when
    // the model/key is null or the key is absent. There is no fallback to the
    // variant slug — callers that want one read transcribe_model_variant_string.
    if (model == nullptr || key == nullptr) {
        return "";
    }
    const auto it = model->meta.find(key);
    return it != model->meta.end() ? it->second.c_str() : "";
}

static int transcribe_tokenize_impl(const struct transcribe_model * model,
                                    const char *                    text,
                                    int32_t *                       tokens,
                                    size_t                          n_max) {
    if (model == nullptr || text == nullptr) {
        return INT_MIN;
    }
    const transcribe::Tokenizer * tok = model->tokenizer();
    if (tok == nullptr) {
        return INT_MIN;
    }

    std::vector<int32_t> ids;
    if (tok->encode(std::string(text), ids) != TRANSCRIBE_OK) {
        return INT_MIN;
    }

    const size_t n = ids.size();
    if (n > static_cast<size_t>(INT_MAX)) {
        return INT_MIN;
    }
    if (n > n_max) {
        // Negative-of-N retry contract (mirrors whisper.cpp). Guard
        // against -n overflowing int range for pathologically large
        // vocabularies; fall back to INT_MIN + 1 so the caller still
        // gets a hard-failure code rather than a stale pointer.
        if (n > static_cast<size_t>(INT_MAX)) {
            return INT_MIN;
        }
        return -static_cast<int>(n);
    }
    if (tokens == nullptr && n > 0) {
        return INT_MIN;
    }
    for (size_t i = 0; i < n; ++i) {
        tokens[i] = ids[i];
    }
    return static_cast<int>(n);
}

extern "C" const char * transcribe_model_backend(const struct transcribe_model * model) {
    // Empty string means "no runtime backend bound" — see the public header
    // for the full semantic.
    if (model == nullptr) {
        return "";
    }
    return model->backend.c_str();
}

static transcribe_status transcribe_model_get_device_impl(const struct transcribe_model *    model,
                                                          struct transcribe_backend_device * out) {
    if (model == nullptr || out == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (const auto st = check_struct_size(out->struct_size, k_min_backend_device_size); st != TRANSCRIBE_OK) {
        return st;
    }
    // The model's primary backend is bound by per-family load(); a model
    // that never resolved one has none.
    if (model->primary_backend == nullptr) {
        return TRANSCRIBE_ERR_BACKEND;
    }
    ggml_backend_dev_t dev = ggml_backend_get_device(model->primary_backend);
    if (dev == nullptr) {
        return TRANSCRIBE_ERR_BACKEND;
    }
    fill_backend_device(dev, out->struct_size, out);
    return TRANSCRIBE_OK;
}

// Timings
//
// Accumulators live on the base classes (transcribe_model::t_load_us and
// transcribe_session::t_{mel,encode,decode}_us), populated by per-family
// load() / run(). These accessors convert microseconds to milliseconds.

extern "C" transcribe_status transcribe_get_timings(const struct transcribe_session * session,
                                                    struct transcribe_timings *       out_timings) {
    if (session == nullptr || out_timings == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (const auto st = check_struct_size(out_timings->struct_size, k_min_timings_size); st != TRANSCRIBE_OK) {
        return st;
    }
    const uint64_t     caller_size = out_timings->struct_size;
    transcribe_timings staged{};
    staged.struct_size = caller_size;
    if (session->model != nullptr) {
        staged.load_ms = static_cast<float>(session->model->t_load_us) / 1000.0f;
    }
    staged.mel_ms    = static_cast<float>(session->t_mel_us) / 1000.0f;
    staged.encode_ms = static_cast<float>(session->t_encode_us) / 1000.0f;
    staged.decode_ms = static_cast<float>(session->t_decode_us) / 1000.0f;
    copy_out_prefix(out_timings, &staged, caller_size, sizeof(staged));
    return TRANSCRIBE_OK;
}

extern "C" void transcribe_print_timings(const struct transcribe_session * session) {
    if (session == nullptr) {
        return;
    }
    struct transcribe_timings t;
    transcribe_timings_init(&t);
    (void) transcribe_get_timings(session, &t);
    char buf[256];

    std::snprintf(buf, sizeof(buf),
                  "timings: load=%.2f ms  mel=%.2f ms  "
                  "encode=%.2f ms  decode=%.2f ms",
                  t.load_ms, t.mel_ms, t.encode_ms, t.decode_ms);
    // Surfaced even in the common dev case (transcribe-cli without an
    // installed sink) via the stderr fallback; an explicit
    // transcribe_log_set(NULL, ...) silences it like everything else.
    transcribe_log_emit_or_stderr(TRANSCRIBE_LOG_LEVEL_INFO, buf);
}

extern "C" void transcribe_reset_timings(struct transcribe_session * session) {
    if (session == nullptr) {
        return;
    }
    // load_us is intentionally left alone — it's a model-scoped fact,
    // not a per-call accumulator.
    session->t_mel_us    = 0;
    session->t_encode_us = 0;
    session->t_decode_us = 0;
}

// Result accessors
//
// These read from the base context's result storage (tokens / words /
// segments / full_text / result_kind / has_result), populated by the
// per-family run() driver. Out-of-range indices and pre-run access return
// safe sentinels per the public contract.

extern "C" const char * transcribe_full_text(const struct transcribe_session * session) {
    if (session == nullptr || !session->has_result) {
        return "";
    }
    return session->full_text.c_str();
}

extern "C" const char * transcribe_detected_language(const struct transcribe_session * session) {
    if (session == nullptr || !session->has_result) {
        return "";
    }
    return session->detected_language.c_str();
}

extern "C" transcribe_timestamp_kind transcribe_returned_timestamp_kind(const struct transcribe_session * session) {
    if (session == nullptr || !session->has_result) {
        return TRANSCRIBE_TIMESTAMPS_NONE;
    }
    return session->result_kind;
}

extern "C" int transcribe_n_segments(const struct transcribe_session * session) {
    if (session == nullptr || !session->has_result) {
        return 0;
    }
    return static_cast<int>(session->segments.size());
}

extern "C" int transcribe_n_words(const struct transcribe_session * session) {
    if (session == nullptr || !session->has_result) {
        return 0;
    }
    return static_cast<int>(session->words.size());
}

extern "C" int transcribe_n_tokens(const struct transcribe_session * session) {
    if (session == nullptr || !session->has_result) {
        return 0;
    }
    return static_cast<int>(session->tokens.size());
}

// Result accessors - per-item rows
//
// Copy-out accessors backed by the context's segments / words / tokens
// vectors, writing only the prefix that fits the caller's struct_size.
// Out-of-range index or pre-run access leaves the caller's struct zero-init
// and still returns OK, so callers can use text!=NULL as the "row present"
// signal without branching on status.
//
// `text` pointers in the staged structs alias the context's std::string
// storage; the library treats that as session-owned, valid until the next
// transcribe_run / transcribe_stream_begin / transcribe_stream_reset /
// transcribe_session_free on the same context (see the public header).

extern "C" transcribe_status transcribe_get_segment(const struct transcribe_session * session,
                                                    int                               i,
                                                    struct transcribe_segment *       out) {
    if (out == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (const auto st = check_struct_size(out->struct_size, k_min_segment_size); st != TRANSCRIBE_OK) {
        return st;
    }
    const uint64_t     caller_size = out->struct_size;
    transcribe_segment zero{};
    zero.struct_size = caller_size;
    copy_out_prefix(out, &zero, caller_size, sizeof(zero));
    if (session == nullptr || !session->has_result || i < 0 || static_cast<size_t>(i) >= session->segments.size()) {
        return TRANSCRIBE_OK;
    }
    const auto &       s = session->segments[static_cast<size_t>(i)];
    transcribe_segment staged{};
    staged.struct_size = caller_size;
    staged.t0_ms       = s.t0_ms;
    staged.t1_ms       = s.t1_ms;
    staged.first_word  = s.first_word;
    staged.n_words     = s.n_words;
    staged.first_token = s.first_token;
    staged.n_tokens    = s.n_tokens;
    staged.text        = s.text.c_str();
    copy_out_prefix(out, &staged, caller_size, sizeof(staged));
    return TRANSCRIBE_OK;
}

extern "C" transcribe_status transcribe_get_word(const struct transcribe_session * session,
                                                 int                               i,
                                                 struct transcribe_word *          out) {
    if (out == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (const auto st = check_struct_size(out->struct_size, k_min_word_size); st != TRANSCRIBE_OK) {
        return st;
    }
    const uint64_t  caller_size = out->struct_size;
    transcribe_word zero{};
    zero.struct_size = caller_size;
    copy_out_prefix(out, &zero, caller_size, sizeof(zero));
    if (session == nullptr || !session->has_result || i < 0 || static_cast<size_t>(i) >= session->words.size()) {
        return TRANSCRIBE_OK;
    }
    const auto &    w = session->words[static_cast<size_t>(i)];
    transcribe_word staged{};
    staged.struct_size = caller_size;
    staged.t0_ms       = w.t0_ms;
    staged.t1_ms       = w.t1_ms;
    staged.seg_index   = w.seg_index;
    staged.first_token = w.first_token;
    staged.n_tokens    = w.n_tokens;
    staged.text        = w.text.c_str();
    copy_out_prefix(out, &staged, caller_size, sizeof(staged));
    return TRANSCRIBE_OK;
}

extern "C" transcribe_status transcribe_get_token(const struct transcribe_session * session,
                                                  int                               i,
                                                  struct transcribe_token *         out) {
    if (out == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (const auto st = check_struct_size(out->struct_size, k_min_token_size); st != TRANSCRIBE_OK) {
        return st;
    }
    const uint64_t   caller_size = out->struct_size;
    transcribe_token zero{};
    zero.struct_size = caller_size;
    copy_out_prefix(out, &zero, caller_size, sizeof(zero));
    if (session == nullptr || !session->has_result || i < 0 || static_cast<size_t>(i) >= session->tokens.size()) {
        return TRANSCRIBE_OK;
    }
    const auto &     t = session->tokens[static_cast<size_t>(i)];
    transcribe_token staged{};
    staged.struct_size = caller_size;
    staged.id          = t.id;
    staged.p           = t.p;
    staged.t0_ms       = t.t0_ms;
    staged.t1_ms       = t.t1_ms;
    staged.seg_index   = t.seg_index;
    staged.word_index  = t.word_index;
    staged.text        = t.text.c_str();
    copy_out_prefix(out, &staged, caller_size, sizeof(staged));
    return TRANSCRIBE_OK;
}

// Batch result accessors
//
// These index session->batch_results when a batch run populated it, and
// otherwise synthesize utterance 0 from the scratch slot after
// transcribe_run. They share the staging shape of the single-result
// accessors above; only the source vector differs.

namespace {

// Non-owning view of one utterance's result. valid == false means "no such
// utterance / empty result" and the accessors return their safe sentinels.
struct BatchResultView {
    const std::vector<transcribe_session::TokenEntry> *   tokens            = nullptr;
    const std::vector<transcribe_session::WordEntry> *    words             = nullptr;
    const std::vector<transcribe_session::SegmentEntry> * segments          = nullptr;
    const std::string *                                   full_text         = nullptr;
    const std::string *                                   detected_language = nullptr;
    transcribe_timestamp_kind                             result_kind       = TRANSCRIBE_TIMESTAMPS_NONE;
    bool                                                  valid             = false;
};

int batch_result_count(const transcribe_session * s) {
    if (s == nullptr) {
        return 0;
    }
    if (!s->batch_results.empty()) {
        return static_cast<int>(s->batch_results.size());
    }
    return s->has_result ? 1 : 0;
}

BatchResultView batch_result_view(const transcribe_session * s, int i) {
    BatchResultView v;
    if (s == nullptr || i < 0) {
        return v;
    }
    if (!s->batch_results.empty()) {
        if (static_cast<size_t>(i) >= s->batch_results.size()) {
            return v;
        }
        const auto & rs = s->batch_results[static_cast<size_t>(i)];
        if (!rs.has_result) {
            return v;  // individually-failed / empty utterance
        }
        v.tokens            = &rs.tokens;
        v.words             = &rs.words;
        v.segments          = &rs.segments;
        v.full_text         = &rs.full_text;
        v.detected_language = &rs.detected_language;
        v.result_kind       = rs.result_kind;
        v.valid             = true;
        return v;
    }
    if (i == 0 && s->has_result) {
        v.tokens            = &s->tokens;
        v.words             = &s->words;
        v.segments          = &s->segments;
        v.full_text         = &s->full_text;
        v.detected_language = &s->detected_language;
        v.result_kind       = s->result_kind;
        v.valid             = true;
    }
    return v;
}

}  // namespace

extern "C" int transcribe_batch_n_results(const struct transcribe_session * session) {
    return batch_result_count(session);
}

extern "C" transcribe_status transcribe_batch_status(const struct transcribe_session * session, int i) {
    if (session == nullptr || i < 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (!session->batch_results.empty()) {
        if (static_cast<size_t>(i) >= session->batch_results.size()) {
            return TRANSCRIBE_ERR_INVALID_ARG;
        }
        return session->batch_results[static_cast<size_t>(i)].status;
    }
    if (i == 0 && session->has_result) {
        return TRANSCRIBE_OK;
    }
    return TRANSCRIBE_ERR_INVALID_ARG;
}

extern "C" const char * transcribe_batch_full_text(const struct transcribe_session * session, int i) {
    const BatchResultView v = batch_result_view(session, i);
    return v.valid ? v.full_text->c_str() : "";
}

extern "C" const char * transcribe_batch_detected_language(const struct transcribe_session * session, int i) {
    const BatchResultView v = batch_result_view(session, i);
    return v.valid ? v.detected_language->c_str() : "";
}

extern "C" transcribe_timestamp_kind transcribe_batch_returned_timestamp_kind(const struct transcribe_session * session,
                                                                              int                               i) {
    const BatchResultView v = batch_result_view(session, i);
    return v.valid ? v.result_kind : TRANSCRIBE_TIMESTAMPS_NONE;
}

extern "C" int transcribe_batch_n_segments(const struct transcribe_session * session, int i) {
    const BatchResultView v = batch_result_view(session, i);
    return v.valid ? static_cast<int>(v.segments->size()) : 0;
}

extern "C" int transcribe_batch_n_words(const struct transcribe_session * session, int i) {
    const BatchResultView v = batch_result_view(session, i);
    return v.valid ? static_cast<int>(v.words->size()) : 0;
}

extern "C" int transcribe_batch_n_tokens(const struct transcribe_session * session, int i) {
    const BatchResultView v = batch_result_view(session, i);
    return v.valid ? static_cast<int>(v.tokens->size()) : 0;
}

extern "C" transcribe_status transcribe_batch_get_segment(const struct transcribe_session * session,
                                                          int                               i,
                                                          int                               j,
                                                          struct transcribe_segment *       out) {
    if (out == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (const auto st = check_struct_size(out->struct_size, k_min_segment_size); st != TRANSCRIBE_OK) {
        return st;
    }
    const uint64_t     caller_size = out->struct_size;
    transcribe_segment zero{};
    zero.struct_size = caller_size;
    copy_out_prefix(out, &zero, caller_size, sizeof(zero));
    const BatchResultView v = batch_result_view(session, i);
    if (!v.valid || j < 0 || static_cast<size_t>(j) >= v.segments->size()) {
        return TRANSCRIBE_OK;
    }
    const auto &       s = (*v.segments)[static_cast<size_t>(j)];
    transcribe_segment staged{};
    staged.struct_size = caller_size;
    staged.t0_ms       = s.t0_ms;
    staged.t1_ms       = s.t1_ms;
    staged.first_word  = s.first_word;
    staged.n_words     = s.n_words;
    staged.first_token = s.first_token;
    staged.n_tokens    = s.n_tokens;
    staged.text        = s.text.c_str();
    copy_out_prefix(out, &staged, caller_size, sizeof(staged));
    return TRANSCRIBE_OK;
}

extern "C" transcribe_status transcribe_batch_get_word(const struct transcribe_session * session,
                                                       int                               i,
                                                       int                               j,
                                                       struct transcribe_word *          out) {
    if (out == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (const auto st = check_struct_size(out->struct_size, k_min_word_size); st != TRANSCRIBE_OK) {
        return st;
    }
    const uint64_t  caller_size = out->struct_size;
    transcribe_word zero{};
    zero.struct_size = caller_size;
    copy_out_prefix(out, &zero, caller_size, sizeof(zero));
    const BatchResultView v = batch_result_view(session, i);
    if (!v.valid || j < 0 || static_cast<size_t>(j) >= v.words->size()) {
        return TRANSCRIBE_OK;
    }
    const auto &    w = (*v.words)[static_cast<size_t>(j)];
    transcribe_word staged{};
    staged.struct_size = caller_size;
    staged.t0_ms       = w.t0_ms;
    staged.t1_ms       = w.t1_ms;
    staged.seg_index   = w.seg_index;
    staged.first_token = w.first_token;
    staged.n_tokens    = w.n_tokens;
    staged.text        = w.text.c_str();
    copy_out_prefix(out, &staged, caller_size, sizeof(staged));
    return TRANSCRIBE_OK;
}

extern "C" transcribe_status transcribe_batch_get_token(const struct transcribe_session * session,
                                                        int                               i,
                                                        int                               j,
                                                        struct transcribe_token *         out) {
    if (out == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (const auto st = check_struct_size(out->struct_size, k_min_token_size); st != TRANSCRIBE_OK) {
        return st;
    }
    const uint64_t   caller_size = out->struct_size;
    transcribe_token zero{};
    zero.struct_size = caller_size;
    copy_out_prefix(out, &zero, caller_size, sizeof(zero));
    const BatchResultView v = batch_result_view(session, i);
    if (!v.valid || j < 0 || static_cast<size_t>(j) >= v.tokens->size()) {
        return TRANSCRIBE_OK;
    }
    const auto &     t = (*v.tokens)[static_cast<size_t>(j)];
    transcribe_token staged{};
    staged.struct_size = caller_size;
    staged.id          = t.id;
    staged.p           = t.p;
    staged.t0_ms       = t.t0_ms;
    staged.t1_ms       = t.t1_ms;
    staged.seg_index   = t.seg_index;
    staged.word_index  = t.word_index;
    staged.text        = t.text.c_str();
    copy_out_prefix(out, &staged, caller_size, sizeof(staged));
    return TRANSCRIBE_OK;
}

extern "C" transcribe_status transcribe_batch_get_timings(const struct transcribe_session * session,
                                                          int                               i,
                                                          struct transcribe_timings *       out) {
    if (session == nullptr || out == nullptr || i < 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (const auto st = check_struct_size(out->struct_size, k_min_timings_size); st != TRANSCRIBE_OK) {
        return st;
    }
    int64_t mel_us = 0, enc_us = 0, dec_us = 0;
    if (!session->batch_results.empty()) {
        if (static_cast<size_t>(i) >= session->batch_results.size()) {
            return TRANSCRIBE_ERR_INVALID_ARG;
        }
        const auto & rs = session->batch_results[static_cast<size_t>(i)];
        mel_us          = rs.t_mel_us;
        enc_us          = rs.t_encode_us;
        dec_us          = rs.t_decode_us;
    } else if (i == 0) {
        mel_us = session->t_mel_us;
        enc_us = session->t_encode_us;
        dec_us = session->t_decode_us;
    } else {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    const uint64_t     caller_size = out->struct_size;
    transcribe_timings staged{};
    staged.struct_size = caller_size;
    if (session->model != nullptr) {
        staged.load_ms = static_cast<float>(session->model->t_load_us) / 1000.0f;
    }
    staged.mel_ms    = static_cast<float>(mel_us) / 1000.0f;
    staged.encode_ms = static_cast<float>(enc_us) / 1000.0f;
    staged.decode_ms = static_cast<float>(dec_us) / 1000.0f;
    copy_out_prefix(out, &staged, caller_size, sizeof(staged));
    return TRANSCRIBE_OK;
}

// C ABI forwarders. Entry points that allocate, reach ggml/driver state,
// or transfer ownership route through api_guard_* here. Other public symbols
// above are kept nothrow by construction: pure POD/c_str reads, prefix
// copies, plain stores, or contained callback emission.

extern "C" transcribe_status transcribe_init_backends(const char * artifact_dir) {
    return api_guard_status("transcribe_init_backends", [&] { return transcribe_init_backends_impl(artifact_dir); });
}

extern "C" transcribe_status transcribe_init_backends_default(void) {
    return api_guard_status("transcribe_init_backends_default",
                            [&] { return transcribe_init_backends_default_impl(); });
}

extern "C" int transcribe_backend_device_count(void) {
    return api_guard_value("transcribe_backend_device_count", 0,
                           [&] { return transcribe_backend_device_count_impl(); });
}

extern "C" transcribe_status transcribe_get_backend_device(int index, struct transcribe_backend_device * out) {
    return api_guard_status("transcribe_get_backend_device",
                            [&] { return transcribe_get_backend_device_impl(index, out); });
}

extern "C" bool transcribe_backend_available(transcribe_backend_request kind) {
    // Raw read before any enum-typed load (see enum_field_raw): the lambda
    // must not read `kind` as an enum, or an unknown probe value is UB.
    const int raw = enum_field_raw(&kind);
    return api_guard_value("transcribe_backend_available", false,
                           [&] { return transcribe_backend_available_impl(raw); });
}

extern "C" transcribe_status transcribe_model_load_file(const char *                                path,
                                                        const struct transcribe_model_load_params * params,
                                                        struct transcribe_model **                  out_model) {
    const transcribe_status st = api_guard_status(
        "transcribe_model_load_file", [&] { return transcribe_model_load_file_impl(path, params, out_model); });
    // Boundary-owned postcondition: failure => *out_model == NULL.
    if (st != TRANSCRIBE_OK && out_model != nullptr && *out_model != nullptr) {
        transcribe_model_free(*out_model);
        *out_model = nullptr;
    }
    return st;
}

extern "C" void transcribe_model_free(struct transcribe_model * model) {
    api_guard_void("transcribe_model_free", [&] { transcribe_model_free_impl(model); });
}

extern "C" transcribe_status transcribe_session_init(struct transcribe_model *                model,
                                                     const struct transcribe_session_params * params,
                                                     struct transcribe_session **             out_session) {
    const transcribe_status st = api_guard_status(
        "transcribe_session_init", [&] { return transcribe_session_init_impl(model, params, out_session); });
    // Boundary-owned postcondition: failure => *out_session == NULL.
    if (st != TRANSCRIBE_OK && out_session != nullptr && *out_session != nullptr) {
        transcribe_session_free(*out_session);
        *out_session = nullptr;
    }
    return st;
}

extern "C" void transcribe_session_free(struct transcribe_session * session) {
    api_guard_void("transcribe_session_free", [&] { transcribe_session_free_impl(session); });
}

extern "C" transcribe_status transcribe_open(const char *                                path,
                                             const struct transcribe_model_load_params * load_params,
                                             const struct transcribe_session_params *    session_params,
                                             struct transcribe_session **                out_session) {
    const transcribe_status st = api_guard_status(
        "transcribe_open", [&] { return transcribe_open_impl(path, load_params, session_params, out_session); });
    // Boundary-owned postcondition: failure => *out_session == NULL.
    if (st != TRANSCRIBE_OK && out_session != nullptr && *out_session != nullptr) {
        transcribe_session_free(*out_session);
        *out_session = nullptr;
    }
    return st;
}

extern "C" transcribe_status transcribe_run(struct transcribe_session *          session,
                                            const float *                        pcm,
                                            int                                  n_samples,
                                            const struct transcribe_run_params * params) {
    return api_guard_status("transcribe_run", [&] { return transcribe_run_impl(session, pcm, n_samples, params); });
}

extern "C" transcribe_status transcribe_run_batch(struct transcribe_session *          session,
                                                  const float * const *                pcm,
                                                  const int *                          n_samples,
                                                  int                                  n,
                                                  const struct transcribe_run_params * params) {
    return api_guard_status("transcribe_run_batch",
                            [&] { return transcribe_run_batch_impl(session, pcm, n_samples, n, params); });
}

extern "C" transcribe_status transcribe_stream_begin(struct transcribe_session *             session,
                                                     const struct transcribe_run_params *    run_params,
                                                     const struct transcribe_stream_params * stream_params) {
    return api_guard_status("transcribe_stream_begin",
                            [&] { return transcribe_stream_begin_impl(session, run_params, stream_params); });
}

extern "C" transcribe_status transcribe_stream_feed(struct transcribe_session *       session,
                                                    const float *                     pcm,
                                                    int                               n_samples,
                                                    struct transcribe_stream_update * update) {
    return api_guard_status("transcribe_stream_feed",
                            [&] { return transcribe_stream_feed_impl(session, pcm, n_samples, update); });
}

extern "C" transcribe_status transcribe_stream_finalize(struct transcribe_session *       session,
                                                        struct transcribe_stream_update * update) {
    return api_guard_status("transcribe_stream_finalize",
                            [&] { return transcribe_stream_finalize_impl(session, update); });
}

extern "C" void transcribe_stream_reset(struct transcribe_session * session) {
    api_guard_void("transcribe_stream_reset", [&] { transcribe_stream_reset_impl(session); });
}

extern "C" transcribe_status transcribe_model_get_device(const struct transcribe_model *    model,
                                                         struct transcribe_backend_device * out) {
    return api_guard_status("transcribe_model_get_device",
                            [&] { return transcribe_model_get_device_impl(model, out); });
}

extern "C" int transcribe_tokenize(const struct transcribe_model * model,
                                   const char *                    text,
                                   int32_t *                       tokens,
                                   size_t                          n_max) {
    return api_guard_value("transcribe_tokenize", INT_MIN,
                           [&] { return transcribe_tokenize_impl(model, text, tokens, n_max); });
}
