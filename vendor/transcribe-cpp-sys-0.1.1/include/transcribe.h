/*
 * transcribe.h - public C API for transcribe.cpp
 *
 * One-header public surface. Callers never need to include <ggml.h>.
 *
 * Threading:
 * - transcribe_model_* functions are thread-safe.
 * - A loaded model may be shared across any number of threads: querying it
 *   (capabilities, metadata, feature probes) and creating sessions from it
 *   are safe concurrently.
 * - KNOWN 0.x LIMITATION — concurrent COMPUTE is not yet supported: at most
 *   one transcribe_run / transcribe_run_batch / active stream may be in
 *   flight across ALL sessions of a given model at a time. Sessions share
 *   the model's backend instances and some per-family model state, so
 *   overlapping runs race (observed: corrupted decodes on CPU, command-
 *   buffer failures on Metal). Callers that want parallel transcription
 *   today should load one model per worker; a per-session backend
 *   architecture lifting this restriction is planned. Serialized use of
 *   many sessions on one model (e.g. a session pool behind a mutex) is
 *   fully supported.
 * - A model must outlive every session created from it.
 * - transcribe_model_free() is only valid after all derived contexts have
 *   been freed and no thread is still using the model.
 * - transcribe_session_* functions are not thread-safe.
 * - A session must be used by at most one thread at a time.
 * - A session may be moved between threads if no two threads ever use it
 *   concurrently.
 * - The log callback may be invoked from any thread (including ggml
 *   worker threads); the userdata pointer must be safe for concurrent
 *   access. The callback must not assume it runs on the caller thread.
 * - transcribe_log_set() should be called ONCE at process startup,
 *   before any models are loaded, contexts are created, or worker
 *   threads exist. That is the only supported usage model in 0.x. The
 *   implementation publishes the callback with release semantics so any
 *   thread that later loads it observes both the callback and any state
 *   the installer wrote before publication. This protection covers the
 *   single startup-time install only.
 * - Calling transcribe_log_set() after threads or models exist is
 *   UNSUPPORTED in 0.x. The implementation is data-race-free (the two
 *   atomics ensure no torn reads), but under concurrent or mid-run
 *   reconfiguration the API does NOT guarantee pair-atomic publication
 *   of (callback, userdata), nor that the previous callback/userdata
 *   will not still be invoked after transcribe_log_set returns (an
 *   in-flight emission on another thread may already hold the old pair).
 *   A caller that needs mid-run reconfiguration must externally
 *   synchronize transcribe_log_set callers AND keep every previous
 *   callback target and userdata alive until no thread can still emit
 *   with them.
 *
 * ABI stability (0.x):
 * - This library is pre-1.0. The on-disk ABI MAY break between 0.x minor
 *   releases. Consumers should rebuild against matching headers and
 *   should not assume any layout, enum value, or symbol set is frozen.
 *
 * Exception safety (C ABI):
 * - No C++ exception escapes a public entry point. Allocation failure maps
 *   to TRANSCRIBE_ERR_OOM, backend/driver failure to TRANSCRIBE_ERR_BACKEND,
 *   and free/teardown functions never fail. The log callback receives
 *   exception detail when available.
 *
 * Params (size-aware structs):
 * - Every caller-owned public struct crossing the ABI carries `struct_size`
 *   as field 0, both for inputs (params, family extensions) and outputs
 *   (capabilities, stream updates, timings, family telemetry). The field
 *   is `uint64_t` for platform-invariant layout across 32/64-bit ABIs.
 * - Callers MUST initialize each struct via its transcribe_*_init()
 *   function (or, for family extensions, the family's init function).
 *   The init function fills sensible defaults and stamps `struct_size`.
 *   `{0}` is NOT accepted as a defaults shortcut: a struct with
 *   `struct_size == 0` is rejected with TRANSCRIBE_ERR_BAD_STRUCT_SIZE,
 *   regardless of any other field's value. The only NULL-equivalent
 *   defaults form is a literal NULL pointer where the entry point
 *   accepts one (every public params pointer does).
 * - For input structs the init function spells out the default values
 *   for every field. For output structs the init function sets
 *   `struct_size` and relies on zero-fill for the rest. The general
 *   convention is "zero means absent / unknown / false / none," but
 *   specific fields may use a different sentinel (e.g. a family stream
 *   extension's transcribe_parakeet_stream_ext::att_context_right uses
 *   -1 for "model default" because 0 is a real value). Each field's doc
 *   spells out its sentinel where it differs from the general rule.
 * - sizeof(struct ...) is evaluated in the caller's translation unit,
 *   so struct_size captures the caller's view of the layout. In 0.x
 *   the library requires struct_size >= the minimum prefix needed by the
 *   entry point. New trailing fields can be appended without raising that
 *   minimum when the library can safely treat their absence as zero/default.
 *   A newer caller with extra trailing fields the older library doesn't
 *   know about is accepted, and the library only writes what it knows so
 *   the caller's tail bytes stay as zero-init. Layout-incompatible changes
 *   (reordering, retyping existing fields) are an ABI break regardless and
 *   are allowed in 0.x without a deprecation cycle.
 * - Family-specific knobs are reached via an opaque, kind-tagged
 *   extension pointer (`struct transcribe_ext`). Callers probe whether
 *   the loaded model accepts a given extension kind in a given slot via
 *   transcribe_model_accepts_ext_kind(), then pass a pointer to a typed
 *   family extension struct (declared in include/transcribe/<family>.h)
 *   whose first field embeds `struct transcribe_ext`. NULL family
 *   extension selects family defaults.
 *
 * Results:
 * - Results are owned by the session and exposed via accessor functions.
 *   Calling transcribe_run() replaces the previous result. All accessors
 *   are safe to call before the first transcribe_run() and return safe
 *   sentinels ("", 0, NaN) when no result is present.
 * - All timestamps are int64_t milliseconds, relative to the original
 *   input audio (never to internally padded audio).
 * - All counts and indices are int.
 * - Out-of-bounds indices are programmer error: in debug builds the
 *   library may assert; in release builds accessors return safe sentinels
 *   instead of reading invalid memory. Callers should bounds-check with
 *   the corresponding transcribe_n_*() accessor before indexing.
 *
 * Result text-pointer lifetime:
 * - Every accessor that returns a `const char *` (transcribe_full_text,
 *   transcribe_detected_language, and the `text` field of segment / word
 *   / token rows copied out by transcribe_get_segment / _word / _token)
 *   returns a borrowed pointer aliasing session-owned storage.
 * - Offline path: the pointer is valid until the NEXT call to
 *   transcribe_run / transcribe_stream_begin / transcribe_stream_reset
 *   / transcribe_session_free on the same session. Read it, copy bytes
 *   if you need them past the next mutating call, and otherwise discard.
 * - Streaming path: transcribe_full_text and row `text` pointers are raw
 *   model snapshot views and may be replaced by every feed/finalize call.
 *   Use transcribe_stream_get_text() for the UI-facing committed/tentative
 *   text views. Its committed_text pointer remains stable until the next
 *   stream mutation, and its bytes are append-only for the life of the
 *   stream.
 * - Bindings that marshal-and-copy at the FFI boundary (Python c_char_p,
 *   Go C.GoString, Rust CStr::to_string, etc.) are safe by construction
 *   regardless of the committed/tentative distinction, and remain the
 *   recommended pattern for any binding that surfaces results as owned
 *   strings in the host language.
 */

#ifndef TRANSCRIBE_H
#define TRANSCRIBE_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

/* ----------------------------------------------------------------------- */
/* Version                                                                 */
/* ----------------------------------------------------------------------- */
/*
 * Single source of truth for the native library version. The top-level
 * CMakeLists.txt parses these three macros to set the CMake project version and
 * the shared-library VERSION/SOVERSION, and transcribe_version() (declared
 * below) returns the same MAJOR.MINOR.PATCH string. Bump the library version
 * here. Pre-1.0 the on-disk ABI MAY break between minor releases (see "ABI
 * stability" above); the Python binding pins its package version to this value
 * exactly and refuses to load a native provider that does not match.
 */
#define TRANSCRIBE_VERSION_MAJOR 0
#define TRANSCRIBE_VERSION_MINOR 1
#define TRANSCRIBE_VERSION_PATCH 1

#define TRANSCRIBE_VERSION_STRINGIZE_(x) #x
#define TRANSCRIBE_VERSION_STRINGIZE(x)  TRANSCRIBE_VERSION_STRINGIZE_(x)
#define TRANSCRIBE_VERSION                                                                       \
    TRANSCRIBE_VERSION_STRINGIZE(TRANSCRIBE_VERSION_MAJOR)                                       \
    "." TRANSCRIBE_VERSION_STRINGIZE(TRANSCRIBE_VERSION_MINOR) "." TRANSCRIBE_VERSION_STRINGIZE( \
        TRANSCRIBE_VERSION_PATCH)

/* Monotonic integer form (MAJOR*10000 + MINOR*100 + PATCH) for compile-time
 * comparisons, e.g. `#if TRANSCRIBE_VERSION_NUMBER >= 200`. */
#define TRANSCRIBE_VERSION_NUMBER \
    (TRANSCRIBE_VERSION_MAJOR * 10000 + TRANSCRIBE_VERSION_MINOR * 100 + TRANSCRIBE_VERSION_PATCH)

#ifndef TRANSCRIBE_API
#    if defined(_WIN32) && !defined(__GNUC__)
#        if defined(TRANSCRIBE_STATIC)
/* Static archive (lib build or static consumer): no dllimport/export.
        * A consumer linking the static transcribe.lib must NOT see dllimport
        * or the linker chases __imp_* thunks the archive never provides
        * (LNK2019). The static `transcribe` CMake target propagates this
        * PUBLIC; non-CMake static consumers define it themselves. */
#            define TRANSCRIBE_API
#        elif defined(TRANSCRIBE_BUILD)
#            define TRANSCRIBE_API __declspec(dllexport)
#        else
#            define TRANSCRIBE_API __declspec(dllimport)
#        endif
#    else
#        define TRANSCRIBE_API __attribute__((visibility("default")))
#    endif
#endif

#ifdef __cplusplus
extern "C" {
#endif

/* ----------------------------------------------------------------------- */
/* Status                                                                  */
/* ----------------------------------------------------------------------- */

typedef enum {
    TRANSCRIBE_OK                         = 0,
    TRANSCRIBE_ERR_INVALID_ARG            = 1,
    TRANSCRIBE_ERR_NOT_IMPLEMENTED        = 2,
    TRANSCRIBE_ERR_FILE_NOT_FOUND         = 3,
    TRANSCRIBE_ERR_GGUF                   = 4,
    TRANSCRIBE_ERR_UNSUPPORTED_ARCH       = 5,
    TRANSCRIBE_ERR_UNSUPPORTED_VARIANT    = 6,
    TRANSCRIBE_ERR_OOM                    = 7,
    TRANSCRIBE_ERR_BACKEND                = 8,
    /* Reserved; not currently returned. */
    TRANSCRIBE_ERR_SAMPLE_RATE            = 9,
    TRANSCRIBE_ERR_UNSUPPORTED_LANGUAGE   = 10,
    TRANSCRIBE_ERR_UNSUPPORTED_TASK       = 11,
    /*
     * Returned by transcribe_run when the caller asks for a
     * timestamp granularity finer than the model can produce.
     * Compared against transcribe_capabilities::max_timestamp_kind,
     * which is set per family at load time. AUTO never trips this
     * code: the dispatcher passes AUTO through unconditionally and
     * the per-family run() handler resolves it to the model's
     * maximum when it assembles the result.
     */
    TRANSCRIBE_ERR_UNSUPPORTED_TIMESTAMPS = 12,
    /*
     * Returned by transcribe_run when the caller's abort callback
     * returns true during the run. Partial results from chunks that
     * completed before the abort are preserved on the session and
     * readable via the normal result accessors; transcribe_was_aborted
     * distinguishes partial-from-abort from complete.
     */
    TRANSCRIBE_ERR_ABORTED                = 13,
    /*
     * Returned by every entry point that takes a caller-owned size-
     * aware struct (params, stream params, capabilities out, timings
     * out, family extension) when the supplied struct_size is zero or
     * smaller than the minimum the call path requires. Distinct from
     * INVALID_ARG so bindings can match on "caller forgot to call the
     * transcribe_*_init function" specifically.
     */
    TRANSCRIBE_ERR_BAD_STRUCT_SIZE        = 14,
    /* Reserved; not currently returned. */
    TRANSCRIBE_ERR_UNSUPPORTED_PNC        = 15,
    /* Reserved; not currently returned. */
    TRANSCRIBE_ERR_UNSUPPORTED_ITN        = 16,
    /*
     * Returned by transcribe_run / transcribe_run_batch when the input
     * audio is longer than the loaded model can process in a single
     * decode. Hard-context-cap families (LLM-style decoders: qwen3_asr,
     * canary_qwen, funasr_nano, granite, granite_nar, voxtral, cohere,
     * canary) reject an over-length clip UP FRONT — before the decode
     * (and, where the binding limit is the encoder's positional table,
     * before the encoder) — by comparing the audio's prefill token count
     * against the model's context window. The usable ceiling is published
     * as transcribe_capabilities::max_audio_ms, so a caller can size input
     * before calling rather than discovering the limit on failure.
     *
     * Distinct from INVALID_ARG so a caller can tell "audio too long for
     * this model" from a malformed argument. This is the "couldn't start"
     * signal; the symmetric "started, couldn't finish" outcome is
     * TRANSCRIBE_ERR_OUTPUT_TRUNCATED.
     *
     * Chunked / unbounded families normally report max_audio_ms == 0 and
     * have no practical input limit. Whisper and parakeet never return this
     * for length. voxtral_realtime is the exception: it still reports
     * unbounded because its wall is far beyond practical clips, but
     * transcribe_run / transcribe_run_batch return this if input crosses the
     * decoder's absolute position cap.
     *
     * See docs/input-limits.md for the full contract.
     */
    TRANSCRIBE_ERR_INPUT_TOO_LONG         = 17,
    /*
     * Returned by transcribe_run when the decode stopped because it hit
     * the model's context / generation budget BEFORE the model emitted
     * end-of-stream — i.e. the transcript is incomplete. This is the
     * "started, couldn't finish" counterpart to INPUT_TOO_LONG, and it is
     * a hard non-OK status by design: a truncated transcript must not be
     * mistaken for a complete one.
     *
     * The partial transcript IS preserved and readable through the normal
     * result accessors (transcribe_full_text, segments, words, tokens),
     * exactly like TRANSCRIBE_ERR_ABORTED — a caller that wants the partial
     * output reads it after checking the status. transcribe_was_truncated()
     * remains as supplemental state (it is true whenever this status is
     * returned). Unlike INPUT_TOO_LONG this cannot be predicted from input
     * length (it depends on how long the transcript runs), so it is
     * reported after the fact rather than gated up front.
     *
     * In transcribe_run_batch this is a PER-UTTERANCE status: the whole-
     * batch call still returns TRANSCRIBE_OK and the truncated utterance
     * carries this code in transcribe_batch_status(session, i), the same
     * way per-utterance INVALID_ARG / INPUT_TOO_LONG are reported.
     *
     * Streaming does NOT use this code: an active stream is incremental and
     * has its own terminal-state machine (see transcribe_stream_*).
     * See docs/input-limits.md for the full contract.
     */
    TRANSCRIBE_ERR_OUTPUT_TRUNCATED       = 18,
} transcribe_status;

/*
 * Returns a statically allocated, NUL-terminated description of a status
 * code. The returned pointer must not be freed and is valid for the
 * lifetime of the process.
 *
 * The parameter is `int` (not `transcribe_status`) for two reasons:
 *   1. It matches the convention of `strerror(int)` and lets callers pass
 *      an arbitrary integer without forming a bogus enum value.
 *   2. Casting an out-of-range integer to a C++ enum without a fixed
 *      underlying type is undefined behavior. Taking an int means the
 *      "out of range returns 'unknown status'" contract can be exercised
 *      portably without tripping UBSan.
 *
 * Any value of transcribe_status converts implicitly to int at the call
 * site, so existing C and C++ callers continue to compile unchanged.
 */
TRANSCRIBE_API const char * transcribe_status_string(int status);

/* ----------------------------------------------------------------------- */
/* Version                                                                 */
/* ----------------------------------------------------------------------- */
/*
 * Runtime version of the loaded native library. The numeric components are
 * also available at compile time as TRANSCRIBE_VERSION_MAJOR/MINOR/PATCH (and
 * the composed string as TRANSCRIBE_VERSION) near the top of this header.
 *
 * Both return borrowed pointers into static storage: never free them, and
 * treat them as valid for the life of the process.
 */
/* "MAJOR.MINOR.PATCH", e.g. "0.1.0". Equals the TRANSCRIBE_VERSION macro the
 * caller compiled against; a mismatch means the header and the linked library
 * disagree. */
TRANSCRIBE_API const char * transcribe_version(void);
/* Short git commit the library was built from, or "unknown" when the build
 * tree carried no git metadata (e.g. an unpacked source tarball). */
TRANSCRIBE_API const char * transcribe_version_commit(void);

/* ----------------------------------------------------------------------- */
/* ABI metadata                                                            */
/* ----------------------------------------------------------------------- */
/*
 * The native library's sizeof/alignof for the public structs that cross the
 * ABI. A binding (Python ctypes, Rust, Swift, ...) declares its own view of
 * each struct, then verifies that view against these values before it
 * constructs any real instance. This is the safe alternative the "Params"
 * section calls for: query the size up front instead of calling a *_init()
 * function on a buffer that might be smaller than the library expects.
 *
 * `which` selects a struct by transcribe_abi_struct. An unknown id (e.g. a
 * newer binding asking an older library about a struct it predates) returns 0,
 * which the binding MUST treat as "cannot verify," never as "size 0." The enum
 * is append-only; do not renumber existing values.
 */
typedef enum {
    TRANSCRIBE_ABI_MODEL_LOAD_PARAMS = 0,
    TRANSCRIBE_ABI_SESSION_PARAMS    = 1,
    TRANSCRIBE_ABI_RUN_PARAMS        = 2,
    TRANSCRIBE_ABI_STREAM_PARAMS     = 3,
    TRANSCRIBE_ABI_CAPABILITIES      = 4,
    TRANSCRIBE_ABI_TIMINGS           = 5,
    TRANSCRIBE_ABI_SEGMENT           = 6,
    TRANSCRIBE_ABI_WORD              = 7,
    TRANSCRIBE_ABI_TOKEN             = 8,
    TRANSCRIBE_ABI_STREAM_UPDATE     = 9,
    TRANSCRIBE_ABI_STREAM_TEXT       = 10,
    TRANSCRIBE_ABI_SESSION_LIMITS    = 11,
    TRANSCRIBE_ABI_EXT               = 12,
    TRANSCRIBE_ABI_BACKEND_DEVICE    = 13,
} transcribe_abi_struct;

/* sizeof / alignof of the selected public struct, or 0 for an unknown id.
 * For every struct that carries a struct_size field, the size returned here
 * equals the value its transcribe_*_init() stamps into struct_size. */
TRANSCRIBE_API size_t transcribe_abi_struct_size(transcribe_abi_struct which);
TRANSCRIBE_API size_t transcribe_abi_struct_align(transcribe_abi_struct which);

/* ----------------------------------------------------------------------- */
/* Logging                                                                 */
/* ----------------------------------------------------------------------- */

/*
 * This enum is the source of truth for callers. ggml's log levels do NOT
 * share these numeric values (its DEBUG/INFO/WARN/ERROR ordering differs),
 * so the internal pass-through layer maps ggml levels onto this enum
 * before any message reaches the installed callback; callers only ever
 * see these values. CONT marks a fragment continuing the previous message
 * (ggml emits these for progress output); hosts that want one line per
 * message may join CONT fragments onto the preceding text.
 */
typedef enum {
    TRANSCRIBE_LOG_LEVEL_NONE  = 0,
    TRANSCRIBE_LOG_LEVEL_INFO  = 1,
    TRANSCRIBE_LOG_LEVEL_WARN  = 2,
    TRANSCRIBE_LOG_LEVEL_ERROR = 3,
    TRANSCRIBE_LOG_LEVEL_DEBUG = 4,
    TRANSCRIBE_LOG_LEVEL_CONT  = 5, /* continue previous line */
} transcribe_log_level;

typedef void (*transcribe_log_callback)(transcribe_log_level level, const char * msg, void * userdata);

/*
 * Global log sink. Three states:
 *
 *   never called          library messages that warrant surfacing go to
 *                         stderr (the dev/CLI default); ggml's internal
 *                         logging keeps its own stderr default.
 *   cb != NULL            every library message AND ggml's internal
 *                         diagnostics (mapped onto transcribe_log_level)
 *                         route to the callback. Caveat: ggml compiles
 *                         its per-module load-failure chatter OUT of
 *                         release builds, so the reliable "why am I not
 *                         on the GPU?" signals are the one-line device
 *                         summary transcribe_init_backends() emits
 *                         through this sink after each fresh scan, and
 *                         the transcribe_backend_* device accessors.
 *   cb == NULL            logging explicitly disabled: library and ggml
 *                         messages are dropped, nothing goes to stderr.
 *
 * The callback must not throw. If it does anyway, the exception is contained
 * at the emission site, the message is dropped, and a note is written to
 * stderr.
 *
 * Call once at process startup (see the threading contract above).
 */
TRANSCRIBE_API void transcribe_log_set(transcribe_log_callback cb, void * userdata);

/* ----------------------------------------------------------------------- */
/* Task / timestamps                                                       */
/* ----------------------------------------------------------------------- */

typedef enum {
    TRANSCRIBE_TASK_TRANSCRIBE = 0,
    TRANSCRIBE_TASK_TRANSLATE  = 1,
} transcribe_task;

/*
 * Timestamp policy: transcribe_run_params_init() requests NONE for
 * text-first transcription. AUTO is an opt-in "richest supported"
 * mode: it is treated as "equal to the model's max_timestamp_kind."
 * The dispatcher never rejects AUTO, and the per-family run() handler
 * resolves it to the finest granularity the model can actually
 * produce when it assembles the result. A non-AUTO request is treated
 * as a ceiling: if the request is finer than the model's max,
 * transcribe_run returns TRANSCRIBE_ERR_UNSUPPORTED_TIMESTAMPS. If the
 * request is coarser-or-equal, the family handler emits only that
 * granularity and any finer per-run data is elided. The actual
 * granularity returned by a run is reported by
 * transcribe_returned_timestamp_kind(session).
 */
typedef enum {
    TRANSCRIBE_TIMESTAMPS_NONE    = 0,
    TRANSCRIBE_TIMESTAMPS_AUTO    = 1,
    TRANSCRIBE_TIMESTAMPS_SEGMENT = 2,
    TRANSCRIBE_TIMESTAMPS_WORD    = 3,
    TRANSCRIBE_TIMESTAMPS_TOKEN   = 4,
} transcribe_timestamp_kind;

/*
 * KV cache type for the encoder's flash-attention path.
 *
 * AUTO:  f16 when model weights are quantized, f32 when weights are f32.
 *        Best default — matches the bandwidth profile of the model.
 * F32:   full-precision KV. Maximum accuracy, highest bandwidth.
 * F16:   half-precision KV. Halves attention bandwidth with minimal
 *        precision loss (~3 decimal digits). Best for bandwidth-bound
 *        backends (integrated GPUs, CPU).
 */
typedef enum {
    TRANSCRIBE_KV_TYPE_AUTO = 0,
    TRANSCRIBE_KV_TYPE_F32  = 1,
    TRANSCRIBE_KV_TYPE_F16  = 2,
} transcribe_kv_type;

/*
 * Punctuation + capitalization (PNC) toggle on the run params.
 *
 * Each family that exposes a runtime PNC toggle reads this field; families
 * that do not (whisper, parakeet, ...) ignore it. The dispatcher emits a
 * WARN-level log message when a non-DEFAULT value is set against a model
 * for which transcribe_model_supports(model, TRANSCRIBE_FEATURE_PNC)
 * returns false, then proceeds with the model's default behavior. Use
 * that probe to pre-check.
 *
 *   DEFAULT (0): the family's shipped default. Zero-init via
 *                transcribe_run_params_init() gives this value. The family
 *                picks the value its model card published WER numbers
 *                against (canary: pnc on; others: family-specific).
 *   OFF:         explicitly disable runtime PNC. Supporting families
 *                emit lowercase, de-punctuated text. Non-supporting
 *                families ignore (with WARN).
 *   ON:          explicitly enable runtime PNC. Supporting families
 *                emit punctuated, capitalized text. Non-supporting
 *                families ignore (with WARN).
 */
enum transcribe_pnc_mode {
    TRANSCRIBE_PNC_MODE_DEFAULT = 0,
    TRANSCRIBE_PNC_MODE_OFF     = 1,
    TRANSCRIBE_PNC_MODE_ON      = 2,
};

/*
 * Inverse text normalization (ITN) toggle on the run params.
 *
 * Symmetric semantics to transcribe_pnc_mode but for ITN — the
 * transformation that renders numbers, dates, currencies, etc. in formal
 * form ("twenty twenty four" → "2024", "ten dollars" → "$10"). Each
 * family that exposes a runtime ITN toggle reads this field; families
 * that do not ignore it. Non-DEFAULT values against a model for which
 * transcribe_model_supports(model, TRANSCRIBE_FEATURE_ITN) returns false
 * emit a WARN and proceed with default behavior.
 *
 *   DEFAULT (0): family default. Zero-init gives this value.
 *   OFF:         explicit ITN off. Supporting families emit verbatim
 *                spoken-form text. Non-supporting families ignore (WARN).
 *   ON:          explicit ITN on. Supporting families apply ITN.
 *                Non-supporting families ignore (WARN).
 *
 * Some families bundle PNC and ITN (e.g. their ITN toggle also flips
 * punctuation/casing as a side effect). See the family doc for the
 * observed bundling.
 */
enum transcribe_itn_mode {
    TRANSCRIBE_ITN_MODE_DEFAULT = 0,
    TRANSCRIBE_ITN_MODE_OFF     = 1,
    TRANSCRIBE_ITN_MODE_ON      = 2,
};

/* ----------------------------------------------------------------------- */
/* Handles                                                                 */
/* ----------------------------------------------------------------------- */

struct transcribe_model;
struct transcribe_session;

/* ----------------------------------------------------------------------- */
/* Family extensions                                                       */
/* ----------------------------------------------------------------------- */

/*
 * Generic header for every typed family extension struct. The kind tag
 * names the family schema (one FourCC-style 32-bit value per schema,
 * allocated in docs/extension-kinds.md); the size is the caller's
 * sizeof of the full extension struct, captured in the caller's
 * translation unit so a newer library only reads what fits.
 *
 * Each family declares its extension struct in
 * include/transcribe/<family>.h with `struct transcribe_ext ext` as
 * field 0 and a transcribe_<family>_<name>_ext_init() function that
 * fills `ext.size`, `ext.kind`, and per-field defaults from the
 * caller's perspective. `ext.size` is `uint64_t` so the family
 * extension layout is platform-invariant across 32/64-bit ABIs;
 * transcribe_ext_check() also takes `uint64_t` for `min_size`.
 *
 * Pointer-to-first-member is well-defined: because `ext` sits at
 * field 0, the address of the family struct equals the address of its
 * embedded transcribe_ext, and family handlers can cast the generic
 * pointer back to the typed family struct after a successful
 * transcribe_ext_check.
 *
 * Include transcribe/extensions.h to pull in every family extension
 * header shipped by this install. Binding generators should usually
 * point at that umbrella header; direct C/C++ callers that only need
 * the generic ABI can keep including transcribe.h. See
 * docs/extension-kinds.md for the registered family extension headers
 * and kind values.
 */
struct transcribe_ext {
    uint64_t size;
    uint32_t kind;
};

/*
 * Validate the universal shape of an extension before the family casts
 * its pointer to the typed struct.
 *
 *   ext == NULL                    -> TRANSCRIBE_OK (family decides
 *                                     what NULL means; usually "defaults").
 *   ext->size < sizeof(transcribe_ext)
 *                                  -> TRANSCRIBE_ERR_BAD_STRUCT_SIZE.
 *   ext->kind != expected_kind     -> TRANSCRIBE_ERR_INVALID_ARG.
 *   ext->size < min_size           -> TRANSCRIBE_ERR_BAD_STRUCT_SIZE.
 *   otherwise                       -> TRANSCRIBE_OK.
 *
 * The family handler owns `expected_kind` (its own TRANSCRIBE_EXT_KIND_*
 * constant) and `min_size` (typically sizeof of the kind's minimum
 * supported layout). The helper exists so every family rejects malformed
 * input identically.
 */
TRANSCRIBE_API transcribe_status transcribe_ext_check(const struct transcribe_ext * ext,
                                                      uint32_t                      expected_kind,
                                                      uint64_t                      min_size);

/*
 * Extension slot — the API surface a typed family extension is pointed
 * at. Slot acceptance is a model-and-slot concern; the dispatcher
 * validates `slot` matches the call site before delegating to the
 * family. A stream-only extension pointed at transcribe_run_params is
 * INVALID_ARG, not silently-ignored. Add new slots by appending to
 * this enum (SESSION / MODEL_LOAD / TOKENIZE would each be one new
 * value, no new probe function); do not renumber.
 */
typedef enum {
    /* transcribe_run_params::family */
    TRANSCRIBE_EXT_SLOT_RUN    = 0,
    /* transcribe_stream_params::family */
    TRANSCRIBE_EXT_SLOT_STREAM = 1,
} transcribe_ext_slot;

/*
 * Returns true when the loaded model variant accepts the named
 * extension kind in the given slot. The probe is per-loaded-model-
 * variant AND per-slot — a family may ship multiple extension kinds
 * that apply to different variants (e.g. parakeet streaming has
 * cache-aware and chunked-attention variants whose stream knobs are
 * different kinds), and the same kind may only be legal on one slot
 * (a streaming extension on the RUN slot is not "ignored"; it is
 * rejected by transcribe_run with INVALID_ARG).
 *
 * Returns false if model is NULL, if the model's family has no
 * extension surface for the requested slot at all, or if the kind is
 * unknown for that slot. The dispatcher's call-site validation always
 * routes through this probe with the correct slot for the entry point.
 *
 * Callers should write intent-first probes ("does this model accept
 * the parakeet stream kind on the STREAM slot?"), not discovery loops
 * over every known kind. See docs/extension-kinds.md for the registered
 * kinds and their slots.
 */
TRANSCRIBE_API bool transcribe_model_accepts_ext_kind(const struct transcribe_model * model,
                                                      transcribe_ext_slot             slot,
                                                      uint32_t                        kind);

/* ----------------------------------------------------------------------- */
/* Params                                                                  */
/* ----------------------------------------------------------------------- */

/*
 * Backend request.
 *
 * AUTO    Pick the best available backend. Takes the first GPU/IGPU
 *         device that successfully initializes in ggml's device
 *         registry order — which is build-time prioritized (Metal on
 *         Apple, Vulkan / CUDA / SYCL on Linux, …). Host-memory
 *         accelerators (BLAS, AMX, …) are additionally layered onto
 *         the scheduler when present — they run on the same memory
 *         as the CPU backend and are orthogonal to the GPU/CPU split.
 *         Always succeeds: CPU is the final fallback when no GPU
 *         initializes.
 *
 * CPU     Strict CPU only. No GPU, no IGPU, and no host-memory
 *         accelerators (BLAS/AMX). This is the right choice for
 *         numerical reference runs, cross-platform determinism, and
 *         for exercising the pure-CPU kernels under test. Always
 *         succeeds.
 *
 * CPU_ACCEL  CPU primary plus host-memory accelerators (BLAS/AMX/…
 *            whatever ggml registers as ACCEL). primary_kind stays
 *            Cpu so any CPU-keyed policy still triggers, but mat-mul-
 *            shaped graph nodes that fit the accel backend's gates
 *            (large F32 src1, ne ≥ 32) get dispatched there. Useful
 *            when GPU is unavailable or undesired but you still want
 *            production-grade CPU throughput. Requires the build to
 *            include the relevant accel backend (e.g. GGML_BLAS=ON).
 *            Falls back to plain CPU semantics if no accel device is
 *            registered. Always succeeds.
 *
 * METAL   Require the Metal backend. Returns TRANSCRIBE_ERR_BACKEND
 *         if Metal is not available in this build. Host-memory
 *         accelerators are still layered on when present.
 *
 * VULKAN  Require the Vulkan backend. Returns TRANSCRIBE_ERR_BACKEND
 *         if Vulkan is not available in this build. Host-memory
 *         accelerators are still layered on when present.
 *
 * Callers that need to know which backend they actually landed on
 * can query transcribe_model_backend() after load.
 */
typedef enum {
    TRANSCRIBE_BACKEND_AUTO      = 0,
    TRANSCRIBE_BACKEND_CPU       = 1,
    TRANSCRIBE_BACKEND_METAL     = 2,
    TRANSCRIBE_BACKEND_VULKAN    = 3,
    TRANSCRIBE_BACKEND_CPU_ACCEL = 4,
    TRANSCRIBE_BACKEND_CUDA      = 5,
} transcribe_backend_request;

/* ----------------------------------------------------------------------- */
/* Backend modules and device discovery                                    */
/* ----------------------------------------------------------------------- */
/*
 * In dynamic-backend builds (GGML_BACKEND_DL, selected by the
 * TRANSCRIBE_GGML_BACKEND_DL build option), compute backends (CPU, Vulkan,
 * CUDA, ...) are separate loadable modules shipped next to the library in a
 * provider artifact directory, not compiled into it. A host (a Python
 * wheel, a Rust crate, an app bundle) loads them ONCE, before the first
 * model load, by pointing the library at that directory. In static builds
 * the compiled-in backends are already registered, and this call is a
 * harmless no-op for a directory containing no modules.
 *
 * The search is strictly package-local: only artifact_dir is scanned —
 * never the executable directory, the working directory, or any system
 * path. (ggml additionally honors the GGML_BACKEND_PATH environment
 * variable as an explicit user override naming one out-of-tree backend
 * module; an unset environment means a fully package-local load.)
 *
 * A module whose system dependencies are missing — e.g. the Vulkan module
 * on a machine with no Vulkan loader, or with a loader but no driver —
 * fails to load quietly and is skipped; the remaining backends keep
 * working. That degradation is the designed behavior: ship Vulkan by
 * default, fall back to CPU on machines that cannot run it. Use the device
 * accessors below to see what actually registered, and
 * transcribe_backend_available() to probe one kind.
 *
 * Idempotent per directory: repeat calls with an already-scanned directory
 * return the SAME status as the first scan without re-loading — including
 * the TRANSCRIBE_ERR_BACKEND case, which is therefore NOT retryable in
 * this process (e.g. installing a driver after the first call requires a
 * new process to pick up). Concurrent calls to this function are
 * serialized against each other; "thread-safe" extends no further — the
 * load mutates the global device registry, so it must complete before any
 * thread calls the device accessors below or loads a model. That ordering
 * is automatic under the supported usage (call once, before the first
 * model load).
 *
 * artifact_dir is UTF-8, like every path crossing this API; on Windows
 * it is converted to a wide path internally.
 *
 * Returns:
 *   TRANSCRIBE_ERR_INVALID_ARG     artifact_dir is NULL or empty.
 *   TRANSCRIBE_ERR_FILE_NOT_FOUND  artifact_dir is not an existing directory.
 *   TRANSCRIBE_ERR_BACKEND         after loading, the process has zero
 *                                  registered compute devices (a dynamic
 *                                  build pointed at a directory with no
 *                                  usable modules: nothing could run).
 *   TRANSCRIBE_OK                  otherwise.
 */
TRANSCRIBE_API transcribe_status transcribe_init_backends(const char * artifact_dir);

/*
 * Package-local default for dynamic-backend builds.
 *
 * In a dynamic-backend build, resolves the directory containing the loaded
 * libtranscribe itself and delegates to transcribe_init_backends(dir). This
 * keeps the search package-local: it does NOT scan the executable directory,
 * current working directory, or system paths. Ship backend modules next to
 * libtranscribe for this helper to find them.
 *
 * In non-dynamic builds the compute backends are compiled in, so this is a
 * no-op returning TRANSCRIBE_OK.
 */
TRANSCRIBE_API transcribe_status transcribe_init_backends_default(void);

/*
 * Number of compute devices currently registered with the runtime
 * (compiled-in backends plus any modules loaded by
 * transcribe_init_backends). A device is something a model can be placed
 * on: the CPU, an Apple GPU via Metal, a Vulkan GPU, ...
 */
TRANSCRIBE_API int transcribe_backend_device_count(void);

/*
 * Device type: ggml's vendor-agnostic classification of a device,
 * orthogonal to `kind` below (which carries the vendor: metal/vulkan/cuda/
 * ...). Backends report this classification themselves, so treat it as a
 * runtime hint about CPU/GPU/IGPU/ACCEL placement rather than a portable
 * hardware-memory taxonomy. The numeric values mirror ggml's device-type
 * enum.
 */
typedef enum {
    TRANSCRIBE_DEVICE_TYPE_CPU   = 0, /* CPU using system memory */
    TRANSCRIBE_DEVICE_TYPE_GPU   = 1, /* backend-reported GPU */
    TRANSCRIBE_DEVICE_TYPE_IGPU  = 2, /* backend-reported integrated GPU */
    TRANSCRIBE_DEVICE_TYPE_ACCEL = 3, /* host-memory accelerator (BLAS/AMX) */
} transcribe_device_type;

/*
 * One registered compute device.
 *
 * name / description / kind / device_id are borrowed pointers into
 * runtime-owned storage: valid for the life of the process, never freed by
 * the caller.
 *
 * kind is the library's vendor classification, one of: "cpu", "accel" (a
 * host-memory accelerator such as BLAS/AMX), "metal", "vulkan", "cuda",
 * "sycl", "gpu" (an unrecognized GPU), or "unknown". device_type is the
 * orthogonal CPU/GPU/IGPU/ACCEL axis.
 *
 * device_id is a stable hardware identifier when the backend reports one
 * (for PCI devices the lower-case bus id "domain:bus:device.function", e.g.
 * "0000:c1:00.0"), or NULL when unknown (e.g. Metal).
 *
 * memory_total is the device's reported capacity in bytes. memory_free is a
 * SNAPSHOT of available bytes at the moment this struct was filled; it goes
 * stale the instant anything allocates — re-call the accessor to refresh it,
 * as every fill re-queries the driver live. Both numbers are backend-defined
 * and NOT comparable across kinds: on Apple unified memory `total` is the
 * recommended max working-set size (not system RAM) and `free` nets out only
 * this process's allocations; on a discrete GPU they are device-global; on
 * the CPU they are system RAM. 0 means the backend does not report it.
 */
struct transcribe_backend_device {
    uint64_t               struct_size;  /* sizeof(*this); set by _init() */
    const char *           name;         /* ggml device name, e.g. "Metal" */
    const char *           description;  /* human-readable, e.g. "Apple M4 Max" */
    const char *           kind;         /* vendor kind string; see above */
    const char *           device_id;    /* stable hw id (PCI bus id) or NULL */
    uint64_t               memory_total; /* reported capacity in bytes, or 0 */
    uint64_t               memory_free;  /* available bytes snapshot, or 0 */
    transcribe_device_type device_type;  /* CPU/GPU/IGPU/ACCEL axis */
};

TRANSCRIBE_API void transcribe_backend_device_init(struct transcribe_backend_device * p);

/*
 * Fill *out (initialized via transcribe_backend_device_init) with device
 * `index` in [0, transcribe_backend_device_count()).
 *
 * memory_free is live as of this call; re-invoke to refresh it (e.g. to
 * poll a device's available memory over time). The device handles are
 * stable for the life of the process, so the same index always names the
 * same device.
 */
TRANSCRIBE_API transcribe_status transcribe_get_backend_device(int index, struct transcribe_backend_device * out);

/*
 * Whether a backend request can be satisfied by some registered device:
 * AUTO whenever any device exists; CPU and CPU_ACCEL when a CPU device
 * exists; METAL / VULKAN / CUDA when a device of that kind exists. Unknown
 * or invalid request values answer false (never an error). This is the
 * probe a binding uses to turn `backend="vulkan"` on a machine without
 * Vulkan into a clear exception instead of a failed model load.
 */
TRANSCRIBE_API bool transcribe_backend_available(transcribe_backend_request kind);

/*
 * Fill *out (initialized via transcribe_backend_device_init) with the
 * compute device this loaded model is running on — the device that owns its
 * weights and runs most of its graph. Same struct and same live-snapshot
 * semantics as transcribe_get_backend_device: memory_free is current as of
 * the call, so re-invoke to ask "how much memory is left on the device my
 * model landed on" at any time after load.
 *
 * Returns TRANSCRIBE_ERR_INVALID_ARG if model or out is NULL (or out fails
 * the struct-size check), or TRANSCRIBE_ERR_BACKEND if the model has no
 * resolved compute device.
 */
TRANSCRIBE_API transcribe_status transcribe_model_get_device(const struct transcribe_model *    model,
                                                             struct transcribe_backend_device * out);

/*
 * Initialization of caller-owned params structs.
 *
 * Every params struct below is initialized by its transcribe_*_init()
 * function, which fills sensible defaults and the struct_size field:
 *
 *     struct transcribe_run_params p;
 *     transcribe_run_params_init(&p);   // defaults + struct_size
 *     p.timestamps = TRANSCRIBE_TIMESTAMPS_WORD;
 *
 * Defaults shortcut: pass NULL wherever a `const struct transcribe_*_params *`
 * is accepted to get pure defaults without declaring a struct at all.
 * NULL is the only defaults form. A struct with `struct_size == 0` —
 * including `struct X p = {0};` — is REJECTED with
 * TRANSCRIBE_ERR_BAD_STRUCT_SIZE; this is deliberate so that
 * uninitialized stack memory whose struct_size byte happens to be zero
 * cannot silently masquerade as "all defaults." The init function (or
 * NULL) is the only safe calling convention.
 *
 * The init functions are one-argument by design: they assume the caller
 * and library agree on the struct layout, which holds for the supported
 * distribution model (the library is built/shipped with its consumers).
 */

/*
 * Model load params.
 *
 * backend:    which backend to request. See transcribe_backend_request
 *             for the semantics of each value. Default is AUTO.
 *
 * gpu_device: Multi-GPU selector. 0 (the default) means "auto / the first
 *             device of the chosen kind": AUTO picks the first GPU/IGPU in
 *             ggml's registry order, and explicit METAL/VULKAN/CUDA requests
 *             pick the first matching device, as before.
 *
 *             A value > 0 selects the GPU/IGPU device at that global ggml
 *             registry index — the same index space transcribe_get_backend_device()
 *             enumerates, so enumerate first to choose one. The selected
 *             device becomes the model's primary backend, validated against
 *             `backend`: it must be a GPU/IGPU, and for an explicit
 *             METAL/VULKAN/CUDA request it must be that vendor. The index is
 *             order-dependent — ggml's registry order can shift across driver
 *             updates or hosts, so treat it as a runtime selection, not a
 *             stable identifier; correlate via the enumerated device's name /
 *             device_id when you need stability.
 *
 *             gpu_device is rejected with TRANSCRIBE_ERR_INVALID_ARG when it
 *             is negative, out of range, names a non-GPU device, names a
 *             device whose vendor doesn't match an explicit GPU request, or
 *             is non-zero alongside a CPU / CPU_ACCEL request (there is no
 *             GPU to select). Note there is no way to explicitly select the
 *             device at registry index 0; that is exactly what AUTO /
 *             first-of-kind already picks.
 */
struct transcribe_model_load_params {
    uint64_t                   struct_size;
    transcribe_backend_request backend;
    int                        gpu_device;
};

TRANSCRIBE_API void transcribe_model_load_params_init(struct transcribe_model_load_params * params);

/*
 * Session init params.
 *
 * n_threads: number of CPU threads for ops that run on CPU. 0 means
 *            "library picks a sensible default".
 *
 * kv_type:   data type for K/V activations in flash attention.
 *            AUTO (default) uses f16 for quantized models, f32 for f32.
 *
 * n_ctx:     optional cap on the decoder context window, in tokens, for
 *            families with a decoder KV cache. It is a memory knob, not an
 *            accuracy knob.
 *              0 (default): use the model's true maximum, read from GGUF
 *                           metadata. This is the right value for almost
 *                           every caller.
 *              > 0:         lower the ceiling to bound KV-cache memory. A
 *                           value above the model maximum is clamped DOWN
 *                           to it — the knob can only narrow, never extend
 *                           past what the model was trained to support.
 *            For decoder-context-bound families (audio-tokens + prompt +
 *            generation share one window), lowering n_ctx lowers the
 *            effective input limit. That effective limit is a per-session
 *            value: read it via transcribe_session_get_limits()
 *            (effective_max_audio_ms), NOT transcribe_capabilities::
 *            max_audio_ms, which is model-level and reflects only the
 *            default context. For encoder-bound families, n_ctx may lower
 *            decoder KV memory / output budget without lowering the input-
 *            audio bound. Ignored by chunked / unbounded families (whisper,
 *            parakeet, voxtral_realtime) — they have no lowerable ceiling, so
 *            a non-zero n_ctx is a no-op there. A negative value returns
 *            TRANSCRIBE_ERR_INVALID_ARG.
 */
struct transcribe_session_params {
    uint64_t           struct_size;
    int                n_threads;
    transcribe_kv_type kv_type;
    int32_t            n_ctx;
};

TRANSCRIBE_API void transcribe_session_params_init(struct transcribe_session_params * params);

/*
 * Per-run params.
 *
 * v1 accepts only 16 kHz mono float32 PCM. There is no caller-supplied
 * sample rate; the library does not link a resampler; the caller is
 * responsible for resampling external audio (e.g. with sox or ffmpeg)
 * before calling transcribe_run. A future revision will introduce a
 * caller-declared input rate, at which point TRANSCRIBE_ERR_SAMPLE_RATE
 * (currently reserved) will become observable.
 *
 * task:        TRANSCRIBE or TRANSLATE. The model must declare support
 *              for translate via its capabilities; otherwise the run
 *              returns TRANSCRIBE_ERR_UNSUPPORTED_TASK.
 *
 * timestamps:  requested granularity. Default params request NONE.
 *              Use AUTO to get the finest granularity the model
 *              supports.
 *
 * pnc:         punctuation+capitalization runtime toggle. See
 *              transcribe_pnc_mode. DEFAULT is always safe. Non-DEFAULT
 *              values against models for which
 *              transcribe_model_supports(model, TRANSCRIBE_FEATURE_PNC) is
 *              false emit a WARN and proceed with the model's default
 *              behavior.
 *
 * itn:         inverse text normalization runtime toggle. See
 *              transcribe_itn_mode. DEFAULT is always safe. Non-DEFAULT
 *              values against models for which
 *              transcribe_model_supports(model, TRANSCRIBE_FEATURE_ITN) is
 *              false emit a WARN and proceed with the model's default
 *              behavior.
 *
 * language:        source language hint as a BCP-47-ish short code, or
 *                  NULL to autodetect (only if the model supports it).
 *
 * target_language: target language for translation tasks, or NULL.
 *
 * String-pointer lifetime (language / target_language): caller-owned, and
 * the library copies what it needs before the API call returns. This holds
 * for transcribe_run / transcribe_run_batch (synchronous) AND for
 * transcribe_stream_begin: the dispatcher copies these strings into
 * session-owned storage at begin, so the caller may free its params —
 * including the strings — the moment begin returns, even though the stream
 * keeps running. Bindings rely on this: ctypes/FFI-managed string buffers
 * die when the begin wrapper returns.
 *
 * keep_special_tags: keep special vocabulary tags (e.g. <|...|>) in the
 *                     returned text fields. Default (false) strips them
 *                     for clean transcripts; set true to keep the raw
 *                     tags. Token-level accessors always expose the raw
 *                     token text regardless of this flag.
 *
 * family:      optional family-specific extension. NULL selects family
 *              defaults. The pointed-to object is caller-owned; the
 *              library copies any values it needs out of it before
 *              transcribe_run returns, so the caller may free the
 *              extension storage immediately after the call. Each
 *              family declares its typed extension struct in
 *              include/transcribe/<family>.h with `struct transcribe_ext
 *              ext` as field 0. Use transcribe_model_accepts_ext_kind
 *              to probe whether the loaded model accepts a given kind
 *              before pointing `family` at it.
 */
struct transcribe_run_params {
    uint64_t struct_size;

    transcribe_task               task;
    transcribe_timestamp_kind     timestamps;
    enum transcribe_pnc_mode      pnc;
    enum transcribe_itn_mode      itn;
    const char *                  language;
    const char *                  target_language;
    bool                          keep_special_tags;
    const struct transcribe_ext * family;

    /*
     * spec_k_drafts: n-gram-lookup speculative-decode draft length for the
     *   offline autoregressive decode step. Family-portable strategy knob;
     *   the family decides how K maps to its internal verify graph.
     *
     *   Convention:
     *     -1: family default (each family picks its tuned K).
     *      0: spec decoding explicitly disabled — standard 1-token-per-step
     *         autoregression. Use this for byte-equal reproduction of
     *         pre-spec behavior or when measuring baseline performance.
     *     >0: draft K tokens per verify pass. Practical range is 1..8;
     *         optimal K is hardware-dependent (compute-bound hardware
     *         prefers small K, bandwidth-bound prefers larger K — see
     *         docs/models/<family>.md for per-family guidance).
     *
     *   Families gate this via transcribe_capabilities::supports_spec_decode.
     *   Setting spec_k_drafts != -1 on a family with
     *   supports_spec_decode == false is silently ignored (the run proceeds
     *   as ordinary autoregression). Probe the capability bit if you want
     *   to know whether the field will take effect.
     */
    int32_t spec_k_drafts;

    /*
     * max_new_tokens: maximum autoregressive transcript tokens to generate
     * before end-of-stream.
     *
     * Convention:
     *    -1: family default.
     *     0: invalid.
     *    >0: caller-specified generation cap. Families still clamp to their
     *        decoder/context capacity.
     */
    int32_t max_new_tokens;
};

TRANSCRIBE_API void transcribe_run_params_init(struct transcribe_run_params * params);

/* ----------------------------------------------------------------------- */
/* Capabilities                                                            */
/* ----------------------------------------------------------------------- */

/*
 * Capabilities are semantic model properties read from GGUF KV (with
 * architecture defaults filling in absent keys). They are immutable
 * after a successful transcribe_model_load_file() and never change
 * based on backend fallback.
 *
 * languages is an array of n_languages NUL-terminated short codes,
 * owned by the model. The pointer is valid until transcribe_model_free.
 *
 * max_timestamp_kind is the finest granularity the family's run()
 * handler can produce. NONE means the model emits text but no
 * alignment data; SEGMENT / WORD / TOKEN expose progressively finer
 * alignment. The dispatcher rejects any transcribe_run request
 * whose timestamps field is finer than this value. AUTO is not
 * validated against this field: the dispatcher passes AUTO through
 * unconditionally and the per-family run() handler resolves AUTO
 * to its own max_timestamp_kind when it assembles the result.
 */
/*
 * Capabilities are split by character, not by size:
 *
 *   - Values and bools-that-gate-allied-data live on
 *     `transcribe_capabilities`. The struct holds data (sample rate,
 *     language list, max timestamp kind, streaming hints) plus the
 *     three "gate" bools whose meaning is inseparable from neighboring
 *     fields (supports_streaming gates the streaming hint trio;
 *     supports_translate gates the task enum and target_language
 *     semantics on transcribe_run_params; supports_language_detect gates
 *     the languages array and autodetect mode).
 *
 *   - Unallied behavioral toggles (initial prompt, temperature
 *     fallback, long-form chunking, cancellation, PNC, ITN, and
 *     anything similar in the future) live behind the
 *     transcribe_model_supports() probe, keyed by transcribe_feature.
 *     A new feature is +1 enum value with no struct_size churn.
 */
struct transcribe_capabilities {
    uint64_t struct_size;

    int32_t                   native_sample_rate;
    int                       n_languages;
    const char * const *      languages;
    transcribe_timestamp_kind max_timestamp_kind;

    /*
     * Gate bools with allied struct data. Kept here so reading the
     * struct gives a coherent picture of "what tasks does this model
     * support and what data does it expose for them."
     *
     *   supports_language_detect: gates the languages[] array's role.
     *     False means the autodetect path is unavailable; the listed
     *     languages (if any) are still the model's supported set when
     *     the caller passes a hint via transcribe_run_params::language.
     *
     *   supports_translate: gates TRANSCRIBE_TASK_TRANSLATE on
     *     transcribe_run_params, plus the meaning of
     *     transcribe_run_params::target_language. Hard-error gate; the
     *     dispatcher rejects TRANSLATE against models with
     *     supports_translate == false.
     *
     *   supports_streaming: gates the streaming entry points
     *     (transcribe_stream_*). Hard-error gate at
     *     transcribe_stream_begin. Streaming configuration and any
     *     latency hints are family-specific and live on the family
     *     stream extension, not on this struct.
     */
    bool supports_language_detect;
    bool supports_translate;
    bool supports_streaming;

    /*
     * supports_spec_decode: gates transcribe_run_params::spec_k_drafts.
     *   True means the family's offline (transcribe_run / transcribe_run_batch)
     *   path implements n-gram-lookup speculative decoding. A non-zero
     *   spec_k_drafts on a model with supports_spec_decode == false is
     *   silently ignored — the run proceeds as ordinary autoregression. This
     *   is a soft gate (no error) because spec is purely a performance
     *   strategy; callers can probe this bit if they want to know whether
     *   passing K will actually do anything.
     */
    bool supports_spec_decode;

    /*
     * Streaming timing hints intentionally do NOT live here. Streaming
     * configuration is irreducibly family-specific (cache-aware
     * right-context frames, buffered (left, chunk, right) tuples,
     * autoregressive decode throttles), and any single generic number
     * is a lossy projection of a family menu whose authoritative,
     * selectable form lives in the family stream extension
     * (include/transcribe/<family>.h) and the family doc
     * (docs/models/<variant>.md). supports_streaming above is the
     * generic gate ("can this model stream?"); "how, and with what
     * latency tradeoffs?" is answered by the family extension. If a real
     * consumer ever needs a generic latency menu, the right shape is a
     * dedicated streaming-preset enumeration query, not flat fields here.
     */

    /*
     * max_audio_ms: the longest audio this model can process in one
     * transcribe_run, in milliseconds of 16 kHz mono input. This is the
     * single number a caller checks to size input before calling.
     *
     *   0  = no practical limit. The family chunks long audio internally
     *        (whisper), is otherwise unbounded by sequence length
     *        (parakeet), or has only an impractically large absolute safety
     *        cap (voxtral_realtime). A family-specific absolute cap may still
     *        reject input past the model's true wall; see docs/input-limits.md.
     *   >0 = a usable ceiling. Its meaning depends on the family's limit
     *        kind, but the number is honest in both cases:
     *          - hard-context-cap families: the bound the library ENFORCES
     *            up front. It is derived from the decoder context window
     *            minus the fixed prompt and a minimum generation reserve.
     *            A longer clip returns TRANSCRIBE_ERR_INPUT_TOO_LONG before
     *            the decode. This is a MODEL-level value reported at the
     *            model's default context (transcribe_session_params::n_ctx
     *            == 0); a session that lowers n_ctx lowers the effective
     *            limit below this number, but this field is not re-derived
     *            per session.
     *          - soft-window families (gigaam, sensevoice, medasr): the
     *            advisory window the model was trained on. Longer input is
     *            accepted but emits a WARN and may be less accurate; it is
     *            not rejected.
     *
     * Zero-init via transcribe_capabilities_init() yields 0; a family that
     * does not set it is therefore reported as unbounded. See
     * docs/input-limits.md for the full contract.
     */
    int64_t max_audio_ms;

    /*
     * translate_target_languages / n_translate_target_languages: the set
     * of target language codes accepted for TRANSCRIBE_TASK_TRANSLATE —
     * the target-side twin of `languages` (which is the valid set for the
     * transcribe-side `language` hint). supports_translate gates whether
     * translation runs at all; this list narrows WHICH targets are valid.
     * A TRANSLATE run whose run_params::target_language is non-NULL and
     * absent from a non-empty list is rejected with
     * TRANSCRIBE_ERR_UNSUPPORTED_LANGUAGE before any decode.
     *
     * n == 0 / NULL is "not advertised" — an information gap, not a claim
     * of zero targets — exactly the convention an empty `languages` uses.
     * GGUFs predating stt.translation.target_languages report 0 here even
     * when supports_translate is true; the gate is then inert and any
     * family-level target/pair checks (e.g. canary's pivot pairs) still
     * apply on top.
     */
    int                  n_translate_target_languages;
    const char * const * translate_target_languages;
};

TRANSCRIBE_API void transcribe_capabilities_init(struct transcribe_capabilities * out);

/*
 * Read model capabilities into caller-owned storage. The caller
 * initializes *out_caps via transcribe_capabilities_init() (zero-fill);
 * the library writes only the fields that fit and leaves tail bytes
 * beyond the caller's struct_size untouched.
 *
 * Returns:
 *   TRANSCRIBE_ERR_INVALID_ARG     model or out_caps is NULL.
 *   TRANSCRIBE_ERR_BAD_STRUCT_SIZE out_caps->struct_size is 0 or
 *                                  smaller than the library's minimum.
 *
 * Pointer fields written by the library (e.g. `languages`) point at
 * model-owned storage and remain valid until transcribe_model_free().
 * The output struct itself is caller-owned.
 */
TRANSCRIBE_API transcribe_status transcribe_model_get_capabilities(const struct transcribe_model *  model,
                                                                   struct transcribe_capabilities * out_caps);

/*
 * Per-feature capability probe. Pure yes/no advisories that don't
 * gate any allied struct data live here instead of as boolean fields
 * on transcribe_capabilities. Adding a feature is +1 enum value with
 * no struct_size advance, which makes the surface easy to grow
 * without ABI churn.
 *
 * Feature meanings:
 *
 *   INITIAL_PROMPT       The model accepts a free-text or token
 *                        prompt to bias decoding. Today: whisper
 *                        only; reached via transcribe_whisper_run_ext.
 *
 *   TEMPERATURE_FALLBACK The model runs a multi-tier temperature loop
 *                        with metric-driven fallback. Today: whisper.
 *
 *   LONG_FORM            The model exposes a long-form chunker that
 *                        handles audio longer than its native window
 *                        in a single transcribe_run call. Today:
 *                        whisper.
 *
 *   CANCELLATION         transcribe_set_abort_callback fires between
 *                        chunks / decode steps and the family hook
 *                        honors it.
 *
 *   PNC                  The model exposes a runtime toggle for
 *                        punctuation + capitalization via
 *                        transcribe_run_params::pnc. False does NOT mean
 *                        the model produces no PNC — only that the
 *                        caller cannot control whether PNC appears.
 *                        Non-DEFAULT pnc against a model where this
 *                        returns false emits a WARN and proceeds.
 *
 *   ITN                  The model exposes a runtime toggle for
 *                        inverse text normalization via
 *                        transcribe_run_params::itn. Same "can-control
 *                        vs does-emit" distinction as PNC; non-DEFAULT
 *                        itn against an unsupported model warns.
 *
 * Returns false on NULL model or unknown feature enum.
 */
typedef enum {
    TRANSCRIBE_FEATURE_INITIAL_PROMPT       = 0,
    TRANSCRIBE_FEATURE_TEMPERATURE_FALLBACK = 1,
    TRANSCRIBE_FEATURE_LONG_FORM            = 2,
    TRANSCRIBE_FEATURE_CANCELLATION         = 3,
    TRANSCRIBE_FEATURE_PNC                  = 4,
    TRANSCRIBE_FEATURE_ITN                  = 5,
} transcribe_feature;

TRANSCRIBE_API bool transcribe_model_supports(const struct transcribe_model * model, transcribe_feature feature);

/*
 * Stable identifier strings read from GGUF and runtime state.
 *
 *   transcribe_model_arch_string(): the general.architecture key,
 *     e.g. "parakeet". Immutable for the lifetime of the model.
 *
 *   transcribe_model_variant_string(): the stt.variant key, e.g.
 *     "tdt-0.6b-v2". Immutable for the lifetime of the model. May be
 *     an empty string if the underlying GGUF did not carry a variant
 *     and the architecture handler had no default to substitute.
 *
 *   transcribe_model_backend(): the runtime backend currently bound
 *     to this model, e.g. "cpu", "metal", "vulkan", "cuda". This is
 *     the mechanism for detecting CPU fallback when GPU was requested.
 *
 *     Returns an empty string when no runtime backend is currently
 *     bound to the model. This covers both (a) model == NULL, the
 *     safe-sentinel pattern used throughout this header, and (b) a
 *     model that has been loaded but is not yet bound to a runtime
 *     backend - for example, a model whose on-disk metadata and
 *     vocabulary have been parsed but whose compute graph has not
 *     been materialized on any backend yet. Callers can distinguish
 *     the two by null-checking the model pointer.
 *
 *     Backend is a runtime fact, reported separately from the
 *     model's semantic capabilities (see
 *     transcribe_model_get_capabilities), which never change based on
 *     backend state.
 *
 * All three return statically allocated or model-owned strings; the
 * caller must not free them and they remain valid until the model is
 * freed.
 */
TRANSCRIBE_API const char * transcribe_model_arch_string(const struct transcribe_model * model);
TRANSCRIBE_API const char * transcribe_model_variant_string(const struct transcribe_model * model);
TRANSCRIBE_API const char * transcribe_model_backend(const struct transcribe_model * model);

/*
 * Generic GGUF string-metadata getter, modeled on llama_model_meta_val_str.
 * Looks up a scalar-string metadata key written by the converter and returns
 * its value; this is how human-facing identity is read rather than a typed
 * accessor per field. Common keys:
 *
 *   "general.name"         friendly label, e.g. "Whisper Large v3"
 *   "general.license"      SPDX expression, e.g. "apache-2.0" (or "other")
 *   "general.license.name" human-friendly license name
 *   "general.license.link" URL to the license text
 *   "general.author", "general.organization", "general.repo_url", ...
 *
 * Returns a model-owned string (valid until the model is freed; do not free
 * it) or an empty string "" when model is NULL, key is NULL, or the key is
 * absent. Only scalar-string KVs are exposed (numeric hyperparameters and
 * arrays such as the token list are not). There is no fallback to the variant
 * slug — for that, use transcribe_model_variant_string().
 */
TRANSCRIBE_API const char * transcribe_model_meta_val_str(const struct transcribe_model * model, const char * key);

/* ----------------------------------------------------------------------- */
/* Lifecycle                                                               */
/* ----------------------------------------------------------------------- */

/*
 * Load a GGUF model from disk.
 *
 * path is UTF-8, like every path crossing this API; on Windows it is
 * converted to a wide path internally, so non-ASCII paths load
 * correctly regardless of the process ANSI code page.
 *
 * params may be NULL for all library defaults. To customize, initialize
 * a struct with transcribe_model_load_params_init() and set fields. A
 * struct with struct_size == 0 (including `{0}`) is rejected with
 * BAD_STRUCT_SIZE — defaults come from NULL, never from an uninitialized
 * struct. See the "Params" section at the top of this header.
 *
 * On success, *out_model is set and the caller owns it. On failure,
 * *out_model is set to NULL and a non-OK status is returned.
 */
TRANSCRIBE_API transcribe_status transcribe_model_load_file(const char *                                path,
                                                            const struct transcribe_model_load_params * params,
                                                            struct transcribe_model **                  out_model);

/*
 * Free a model. Only valid after every session derived from this model
 * has been freed and no thread is still using the model. Passing NULL
 * is a no-op.
 */
TRANSCRIBE_API void transcribe_model_free(struct transcribe_model * model);

/*
 * Initialize a transcription session bound to a loaded model. Multiple
 * sessions may be created from the same model in parallel; each session
 * is single-threaded. The model must outlive every session derived from
 * it (the session borrows, but does not own, the model).
 *
 * params may be NULL for library defaults, or initialize a struct with
 * transcribe_session_params_init(). See transcribe_model_load_file.
 */
TRANSCRIBE_API transcribe_status transcribe_session_init(struct transcribe_model *                model,
                                                         const struct transcribe_session_params * params,
                                                         struct transcribe_session **             out_session);

/*
 * Free a session.
 *
 * If the session was created via transcribe_open it owns the model it
 * loaded; transcribe_session_free destroys the session and then frees
 * the model in the same call. Sessions created via the two-step
 * transcribe_session_init path borrow their model; this function leaves
 * that model alone.
 *
 * The caller does not need to know which path created the session — both
 * are handled correctly. Passing NULL is a no-op.
 */
TRANSCRIBE_API void transcribe_session_free(struct transcribe_session * session);

/*
 * Convenience: load a model and open a session against it in one call,
 * for the common "I just want to transcribe one stream" case. Bundles
 * transcribe_model_load_file + transcribe_session_init.
 *
 * The returned session OWNS the model it loaded; transcribe_session_free
 * (or its alias transcribe_close) frees both. To share one model across
 * multiple sessions (e.g. one per thread), use the two-step
 * transcribe_model_load_file + transcribe_session_init API instead —
 * that is the only reason to reach past this function.
 *
 * load_params and session_params may each be NULL for library defaults.
 *
 * On success *out_session is set and the caller owns it. On failure
 * *out_session is set to NULL and a non-OK status is returned (the same
 * status transcribe_model_load_file / transcribe_session_init would
 * return; no partial state leaks).
 */
TRANSCRIBE_API transcribe_status transcribe_open(const char *                                path,
                                                 const struct transcribe_model_load_params * load_params,
                                                 const struct transcribe_session_params *    session_params,
                                                 struct transcribe_session **                out_session);

/*
 * Deprecated alias for transcribe_session_free. Both honor owns_model and
 * free the owned model when present, so a caller may use either with any
 * session. New code should call transcribe_session_free directly. Passing
 * NULL is a no-op.
 */
TRANSCRIBE_API void transcribe_close(struct transcribe_session * session);

/*
 * Borrow the model bound to a session. The returned pointer is owned by
 * the library — do not free it — and remains valid for the session's
 * lifetime. This lets convenience-path (transcribe_open) callers reach
 * model introspection such as transcribe_model_backend() and the
 * capabilities query without holding a separate model handle. Returns
 * NULL if session is NULL.
 */
TRANSCRIBE_API const struct transcribe_model * transcribe_get_model(const struct transcribe_session * session);

/* ----------------------------------------------------------------------- */
/* Run                                                                     */
/* ----------------------------------------------------------------------- */

/*
 * Run one batch transcription.
 *
 * pcm:        mono float32 PCM samples in [-1.0, 1.0] at 16 kHz.
 * n_samples:  number of samples in pcm. Must be strictly positive;
 *             a non-positive count returns TRANSCRIBE_ERR_INVALID_ARG
 *             (same rule as transcribe_stream_feed).
 * params:     run params, or NULL for defaults.
 *
 * v1 supports only 16 kHz mono float32 PCM and does not link a
 * resampler; the caller is responsible for resampling external audio
 * before calling this function.
 *
 * On success, results are populated on the session and may be read via
 * the accessors below. Calling transcribe_run() again replaces the
 * previous result on the same session.
 *
 * TRANSCRIBE_ERR_BACKEND from this function (and batch/streaming run calls)
 * is recoverable in-process: free the session and model, reload with
 * transcribe_model_load_params::backend = TRANSCRIBE_BACKEND_CPU, and retry.
 */
TRANSCRIBE_API transcribe_status transcribe_run(struct transcribe_session *          session,
                                                const float *                        pcm,
                                                int                                  n_samples,
                                                const struct transcribe_run_params * params);

/* ----------------------------------------------------------------------- */
/* Batch run (offline)                                                     */
/* ----------------------------------------------------------------------- */

/*
 * Run N offline transcriptions in one call.
 *
 * This is a throughput entry point: families with a batched compute path
 * (today: see the per-family docs) process all N utterances in a single
 * device dispatch, which can roughly double throughput on a GPU that a
 * single utterance underutilizes. Families without a batched path still
 * work — the library falls back to running each utterance in turn — so
 * every model accepts this call; only the speedup is family-dependent.
 *
 * This is the offline counterpart to transcribe_run; it is NOT a streaming
 * primitive. For a well-formed single utterance it is equivalent to
 * transcribe_run with n == 1, but the two are NOT exactly equivalent for
 * malformed input: transcribe_run(NULL pcm / n_samples <= 0) fails top-level
 * with TRANSCRIBE_ERR_INVALID_ARG and leaves the previous result untouched,
 * whereas a malformed single utterance inside a batch (with valid top-level
 * args) is a per-utterance failure — the call returns TRANSCRIBE_OK,
 * transcribe_batch_status(session, 0) carries the INVALID_ARG, and the
 * previous result has already been replaced by the (failed) batch.
 *
 *   session:    a session in a non-ACTIVE-stream state. As with
 *               transcribe_run, a session is single-threaded: do not call
 *               this concurrently with any other call on the same session.
 *   pcm:        array of n pointers, each to mono float32 PCM in [-1, 1]
 *               at 16 kHz. pcm[i] is the i-th utterance.
 *   n_samples:  array of n sample counts; n_samples[i] is the length of
 *               pcm[i].
 *   n:          number of utterances. Must be strictly positive.
 *   params:     run params shared by every utterance in the batch, or
 *               NULL for defaults. v1 applies one params (task, language
 *               hint, timestamps, ...) to the whole batch; per-utterance
 *               params is a future extension.
 *
 * Variable-length utterances are handled by the library: a batched family
 * pads the batch to its longest utterance internally and masks the padding.
 * Grouping utterances of similar length per call ("bucketing") minimizes
 * wasted compute but is not required.
 *
 * Result model: results are read back with the transcribe_batch_* accessors
 * below, indexed by utterance. transcribe_batch_n_results() returns the
 * count. The legacy single-result accessors (transcribe_full_text, etc.)
 * alias utterance 0 after a batch run. A subsequent transcribe_run clears
 * the batch results.
 *
 * Return value is the WHOLE-BATCH status:
 *   TRANSCRIBE_OK                  the dispatch ran. Per-utterance success
 *                                  or failure is read via
 *                                  transcribe_batch_status(session, i); a
 *                                  malformed single utterance (pcm[i] NULL
 *                                  or n_samples[i] <= 0) fails only that
 *                                  utterance and is reported there.
 *   TRANSCRIBE_ERR_INVALID_ARG     session / pcm / n_samples NULL, n <= 0,
 *                                  or the session is in an ACTIVE stream.
 *   TRANSCRIBE_ERR_BAD_STRUCT_SIZE params->struct_size below the minimum.
 *   TRANSCRIBE_ERR_NOT_IMPLEMENTED the model has no run path at all.
 *   ... plus the same shared-param rejections as transcribe_run
 *       (TRANSCRIBE_ERR_UNSUPPORTED_TASK / _TIMESTAMPS / _LANGUAGE), which
 *       apply to the whole batch and are reported before any utterance
 *       runs (the previous result snapshot is preserved on these).
 *   TRANSCRIBE_ERR_ABORTED         the abort callback fired. Completed
 *                                  utterances are retained with their real
 *                                  status; any utterance that does NOT
 *                                  complete reports TRANSCRIBE_ERR_ABORTED,
 *                                  whether it was aborted mid-decode or was
 *                                  never retained as a completed result by the
 *                                  batch path. Dispatcher-synthesized slots
 *                                  specifically mean "did not complete because
 *                                  the batch was aborted" — not that the
 *                                  utterance itself reached an abort
 *                                  checkpoint. As on a successful return,
 *                                  transcribe_batch_n_results is n (one slot
 *                                  per input utterance).
 */
TRANSCRIBE_API transcribe_status transcribe_run_batch(struct transcribe_session *          session,
                                                      const float * const *                pcm,
                                                      const int *                          n_samples,
                                                      int                                  n,
                                                      const struct transcribe_run_params * params);

/* ----------------------------------------------------------------------- */
/* Cancellation                                                            */
/* ----------------------------------------------------------------------- */

/*
 * Abort callback. Polled between chunks AND between decode steps (after
 * each token). Abort latency is therefore one decode step — typically
 * 10-50 ms on CPU, sub-ms on Metal. Mid-encoder abort is not supported
 * today (would require ggml hooks that do not exist).
 *
 * Returning true aborts the in-flight run: transcribe_run returns
 * TRANSCRIBE_ERR_ABORTED and result accessors (transcribe_full_text,
 * transcribe_n_segments, etc.) expose the partial content from chunks
 * that completed before abort. Use transcribe_was_aborted(session) to
 * distinguish partial-from-abort from complete.
 *
 * The next successful transcribe_run() replaces the result atomically,
 * as today.
 *
 * The callback fires on the run thread. If the caller wants to trigger
 * abort from another thread, the common pattern is to store an
 * std::atomic<bool> inside user_data and have the callback load it.
 */
typedef bool (*transcribe_abort_callback)(void * user_data);

/*
 * Install or clear the abort callback for a session. Passing cb=NULL
 * clears any previously installed callback. Safe to call before the
 * first transcribe_run. No-op if session is NULL.
 *
 * Not thread-safe with respect to an in-flight transcribe_run on the
 * same session — the session is single-threaded-at-a-time per the
 * threading contract above. Callers that need to trigger abort from
 * another thread should do so by flipping state inside the callback's
 * user_data, not by swapping the callback itself.
 */
TRANSCRIBE_API void transcribe_set_abort_callback(struct transcribe_session * session,
                                                  transcribe_abort_callback   cb,
                                                  void *                      user_data);

/*
 * True if the most recent transcribe_run was aborted by the installed
 * callback returning true. Reset to false at the top of each
 * transcribe_run. Returns false if session is NULL.
 */
TRANSCRIBE_API bool transcribe_was_aborted(const struct transcribe_session * session);

/*
 * Supplemental flag for output truncation. True if the most recent decode
 * stopped at the model's context / generation cap before end-of-stream,
 * leaving the transcript incomplete. The partial transcript is preserved
 * and readable through the normal result accessors. Reset to false at the
 * start of each new decode — transcribe_run, transcribe_run_batch, and
 * transcribe_stream_begin (the same lifecycle as transcribe_was_aborted).
 * Returns false if session is NULL.
 *
 * Two paths set it, and they differ in whether a status also reports it:
 *
 *   - Offline (transcribe_run / transcribe_run_batch): the flag is true
 *     exactly when the run returned TRANSCRIBE_ERR_OUTPUT_TRUNCATED (or, in
 *     a batch, when a per-utterance status is OUTPUT_TRUNCATED), so the run
 *     status is the authoritative signal and this accessor is a convenience
 *     for a caller that has lost it.
 *
 *   - Streaming (transcribe_stream_*): OUTPUT_TRUNCATED is NOT used. An
 *     active stream has its own terminal-state machine, and stream_feed /
 *     stream_finalize return the status of that step, not a verdict on the
 *     whole transcript — so they return TRANSCRIBE_OK even when the stream
 *     reached its absolute position cap (forcing the stream to FAILED would
 *     discard the committed text the caller has been consuming). There, this
 *     flag is the ONLY signal of truncation: a streaming caller must check
 *     it after finalize.
 *
 * Distinct from the "couldn't start" rejection: input that cannot fit at
 * all is rejected before the decode with TRANSCRIBE_ERR_INPUT_TOO_LONG;
 * output truncation cannot be predicted from input length (it depends on
 * how long the transcript runs), so it is reported after the fact. A WARN
 * is also logged. See docs/input-limits.md.
 */
TRANSCRIBE_API bool transcribe_was_truncated(const struct transcribe_session * session);

/* ----------------------------------------------------------------------- */
/* Session limits                                                          */
/* ----------------------------------------------------------------------- */

/*
 * Effective limits for a specific session. This is the session-level companion
 * to the model-level transcribe_capabilities::max_audio_ms: for families whose
 * audio consumes decoder context, it accounts for the session's
 * transcribe_session_params::n_ctx cap. For encoder-bound families, n_ctx may
 * lower decoder KV memory without lowering the input-audio bound.
 *
 *   effective_n_ctx:        the decoder context cap, in tokens, in force for
 *                           this session: model_max_ctx when n_ctx == 0,
 *                           else min(n_ctx, model_max_ctx). 0 means the
 *                           family has no context cap (chunked / unbounded).
 *
 *   effective_max_audio_ms: the longest audio this session accepts before
 *                           TRANSCRIBE_ERR_INPUT_TOO_LONG, in milliseconds
 *                           of 16 kHz mono input. For decoder-context-bound
 *                           families this is derived from effective_n_ctx; for
 *                           encoder-bound families it remains the encoder
 *                           input bound even when n_ctx lowers decoder memory.
 *                           0 means no practical limit. Advisory /
 *                           representative in the same way as
 *                           transcribe_capabilities::max_audio_ms (it assumes
 *                           a representative prompt; the exact per-call bound
 *                           shifts slightly with the prompt).
 *
 *   max_kv_bytes:           the worst-case decoder KV-cache bytes for ONE
 *                           utterance at effective_n_ctx, for memory
 *                           budgeting. It is exact for the session's kv_type
 *                           (the families resolve AUTO and F16 to f16 KV and
 *                           use f32 only for an explicit F32 request, so this
 *                           reflects 2 or 4 bytes/element accordingly). It is
 *                           the ceiling, not the amount allocated for any
 *                           single run (the cache grows to fit each input).
 *                           One scaling caveat the caller applies itself: it
 *                           is per-utterance, so transcribe_run_batch
 *                           allocates roughly batch_size x this. 0 if the
 *                           family has no decoder KV cache.
 */
struct transcribe_session_limits {
    uint64_t struct_size;
    int32_t  effective_n_ctx;
    int64_t  effective_max_audio_ms;
    int64_t  max_kv_bytes;
};

TRANSCRIBE_API void transcribe_session_limits_init(struct transcribe_session_limits * out);

/*
 * Read the effective limits for a session into caller-owned storage. The
 * caller initializes *out via transcribe_session_limits_init(); the library
 * writes only the prefix that fits in both the caller's struct_size and the
 * library's view.
 *
 * Returns:
 *   TRANSCRIBE_ERR_INVALID_ARG     session or out is NULL.
 *   TRANSCRIBE_ERR_BAD_STRUCT_SIZE out->struct_size is 0 or below the
 *                                  library minimum.
 *   TRANSCRIBE_OK                  otherwise.
 */
TRANSCRIBE_API transcribe_status transcribe_session_get_limits(const struct transcribe_session *  session,
                                                               struct transcribe_session_limits * out);

/* ----------------------------------------------------------------------- */
/* Streaming                                                               */
/* ----------------------------------------------------------------------- */

/*
 * Streaming is a mode on transcribe_session, not a separate handle. A
 * session is in exactly one of four lifecycle states at any time, and
 * the result accessors (transcribe_full_text, segments, words, tokens)
 * return the current raw model snapshot of the active stream.
 *
 * Text semantics during an active stream:
 *
 *   full_text      = raw current model hypothesis
 *   committed_text = append-only UI/input prefix
 *   tentative_text = volatile raw suffix after raw_tentative_start_bytes
 *   display_text   = committed_text + tentative_text
 *
 * The committed-count accessors (transcribe_stream_n_committed_*)
 * remain as low-level raw row boundary hints for consumers that inspect
 * tokens/words/segments. They are monotonic commitment high-water marks
 * and may exceed the current raw row counts after the model revises or
 * shrinks its latest hypothesis; clamp them to transcribe_n_* before
 * indexing current raw rows. UI consumers should prefer
 * transcribe_stream_get_text(), because committed_text is independent from
 * the raw hypothesis and may not be a prefix of full_text after the model
 * revises already-committed text.
 *
 * Streaming timestamp note: unlike transcribe_run(), an active stream
 * may keep token rows populated (transcribe_n_tokens > 0) even when the
 * run was started with timestamps == TRANSCRIBE_TIMESTAMPS_NONE, because
 * the committed/tentative progress boundary is token-indexed and the
 * incremental-commit accessors need the token rows to exist. The
 * timestamps request is validated against the model's capabilities at
 * transcribe_stream_begin (a request finer than the model's max is
 * rejected there), but it is NOT an instruction to elide structural
 * token rows from the streaming result. Whether those rows carry real
 * per-token timestamps is a separate question answered by the family:
 * a family with no token-level alignment (e.g. moonshine_streaming) can
 * honestly populate token rows for the progress boundary while
 * transcribe_returned_timestamp_kind() still reports NONE. Always use
 * transcribe_returned_timestamp_kind() to discover the finest timestamp
 * fields actually populated; do not infer it from transcribe_n_tokens().
 *
 * Cancellation reuses the existing session abort callback. The
 * callback is polled at chunk boundaries and decode-step boundaries
 * during feed/finalize; on cancellation the active call returns
 * TRANSCRIBE_ERR_ABORTED, partial results remain readable, and the
 * stream transitions to FAILED (transcribe_was_aborted distinguishes
 * caller cancellation from other terminal statuses).
 *
 * Multiple concurrent streams: use the existing model/session
 * threading model. Create multiple contexts from one loaded model,
 * then run at most one stream per session. The model is shared and
 * read-only; each session owns its own stream state.
 *
 * Streaming params (transcribe_stream_params) carry an optional,
 * kind-tagged family extension pointer. Family-specific extension
 * structs live in include/transcribe/<family>.h and are reached
 * intent-first: a caller that wants to set a parakeet streaming knob
 * probes transcribe_model_accepts_ext_kind for the parakeet stream
 * kind on TRANSCRIBE_EXT_SLOT_STREAM and, if accepted, points
 * transcribe_stream_params::family at an extension struct initialized
 * via that family's transcribe_<family>_<name>_ext_init() function.
 */

enum transcribe_stream_state {
    TRANSCRIBE_STREAM_IDLE     = 0,
    TRANSCRIBE_STREAM_ACTIVE   = 1,
    TRANSCRIBE_STREAM_FINISHED = 2,
    TRANSCRIBE_STREAM_FAILED   = 3,
};

/*
 * Generic public commitment policy. Families still own model-specific
 * streaming mechanics (chunk sizes, cache windows, decoder throttles);
 * this enum controls when the session's UI-facing committed_text grows.
 *
 * AUTO:        Use the dispatcher-selected stable-prefix implementation
 *              for the model family. Current streaming families use a
 *              family-provided boundary; future families fall back to
 *              the generic text-agreement implementation until they are
 *              assigned an explicit default.
 * ON_FINALIZE: During feed calls committed_text stays empty and the raw
 *              hypothesis is exposed as tentative_text. Finalize commits
 *              the final raw text because no earlier bytes were committed.
 * STABLE_PREFIX:
 *              Commit only a raw prefix accepted by the selected stable-
 *              prefix implementation. Known families may use token-id or
 *              native commit boundaries; others use generic text
 *              agreement. committed_text is append-only and finalize
 *              never rewrites previously committed bytes.
 */
typedef enum {
    TRANSCRIBE_STREAM_COMMIT_AUTO          = 0,
    TRANSCRIBE_STREAM_COMMIT_ON_FINALIZE   = 1,
    TRANSCRIBE_STREAM_COMMIT_STABLE_PREFIX = 2,
} transcribe_stream_commit_policy;

/*
 * Streaming run params.
 *
 * struct_size:                   sizeof(*this) captured by the caller.
 *                                Initialized via transcribe_stream_params_init().
 *
 * family:                        Optional family-specific extension. NULL
 *                                selects family defaults. The pointed-to
 *                                object is caller-owned and the library
 *                                copies any values it needs out of it
 *                                before transcribe_stream_begin returns;
 *                                the caller may free the extension storage
 *                                immediately after begin. Each family
 *                                declares its typed extension struct in
 *                                include/transcribe/<family>.h with
 *                                `struct transcribe_ext ext` as field 0.
 *                                Use transcribe_model_accepts_ext_kind to
 *                                probe whether the loaded model accepts a
 *                                given kind before pointing `family` at it.
 *
 * commit_policy:                 Generic UI-facing commitment policy.
 *                                Zero-init/default is AUTO.
 *
 * stable_prefix_agreement_n:      For STABLE_PREFIX and AUTO implementations
 *                                based on repeated hypotheses, number of
 *                                consecutive text or token-id hypotheses that
 *                                must agree on a prefix before it is appended
 *                                to committed_text. Families with native
 *                                irreversible commit boundaries may ignore
 *                                this field. 0 selects the library default
 *                                (currently 3): a prefix must reproduce
 *                                across three consecutive hypotheses before
 *                                it is committed.
 */
struct transcribe_stream_params {
    uint64_t                        struct_size;
    const struct transcribe_ext *   family;
    transcribe_stream_commit_policy commit_policy;
    uint32_t                        stable_prefix_agreement_n;
};

TRANSCRIBE_API void transcribe_stream_params_init(struct transcribe_stream_params * params);

/*
 * Optional per-call change metadata returned by feed/finalize.
 *
 *   struct_size        Caller's sizeof(*this). Initialized via
 *                      transcribe_stream_update_init() (zero-fill).
 *   result_changed     True when any observable property of the
 *                      snapshot changed on this call: the text vectors
 *                      (tokens / words / segments / full_text), the
 *                      committed/tentative text views, committed raw
 *                      row boundary hints, or a lifecycle transition
 *                      that semantically closes the stream.
 *                      Inspect the accessors after the call to read
 *                      the new snapshot. Treat result_changed as a
 *                      strict subset of `revision changed`: if
 *                      `revision` advanced you can assume
 *                      `result_changed` is also true.
 *   is_final           True only on the finalize call's update. Set
 *                      by the dispatcher after the family hook
 *                      returns; family hooks cannot override.
 *   revision           Monotonic snapshot counter. Advances whenever
 *                      any observable property of the result changes
 *                      under the rules described for `result_changed`,
 *                      including finalize cases where text is unchanged
 *                      but the committed/tentative stream view or
 *                      lifecycle moved. Mirrors
 *                      transcribe_stream_revision(session) after the
 *                      call returns. UI consumers should diff against
 *                      the previous value rather than treating each
 *                      bump as "text changed."
 *   input_received_ms  Total audio received by the stream since begin.
 *   audio_committed_ms Family-reported audio progress / drain hint.
 *                      It is not a byte boundary into committed_text.
 *                      ON_FINALIZE reports 0 during feed calls because
 *                      committed_text is empty until finalize.
 *   buffered_ms        Audio still buffered inside the family's
 *                      streaming state (frontend carry +
 *                      lookahead/right-context requirement). Caller
 *                      may use this as a "drain" hint.
 *   committed_changed True when committed_text changed on this call.
 *                      committed_text is append-only for the life of
 *                      the stream; a true value means bytes were appended
 *                      or, for ON_FINALIZE with no prior committed bytes,
 *                      the final raw text was installed.
 *   tentative_changed True when tentative_text changed on this call.
 *
 * update is nullable. Passing NULL means the caller will inspect the
 * session directly (revision + committed-count accessors). Audio
 * cursor fields are only available via this struct.
 *
 * Zero-means-absent contract: every field on this struct is designed
 * so the zero value encodes "absent / unknown / false / none." This
 * is what makes the transcribe_stream_update_init() zero-fill safe for
 * forward ABI: a new caller paired with an older library reads tail
 * fields the older library never wrote, and those bytes must remain
 * zero.
 */
struct transcribe_stream_update {
    uint64_t struct_size;
    bool     result_changed;
    bool     is_final;
    int32_t  revision;
    int64_t  input_received_ms;
    int64_t  audio_committed_ms;
    int64_t  buffered_ms;
    bool     committed_changed;
    bool     tentative_changed;
};

TRANSCRIBE_API void transcribe_stream_update_init(struct transcribe_stream_update * out);

/*
 * UI-facing stream text snapshot.
 *
 * full_text is the raw current model hypothesis, matching
 * transcribe_full_text(session). During ACTIVE it may rewrite anywhere.
 *
 * committed_text is the API-stable display/input prefix. Once bytes are
 * exposed through committed_text they are never rewritten by later feed or
 * finalize calls on this stream. They remain until begin/reset/free.
 *
 * tentative_text is the volatile raw suffix after raw_tentative_start_bytes.
 * It may be replaced on every feed. UI consumers that want no committed
 * flicker render committed_text + tentative_text; consumers that want the
 * model's current truth render full_text.
 *
 * committed_text is best-effort, not a correctness guarantee. For models
 * that re-attend over a growing audio context (e.g. moonshine_streaming),
 * the raw hypothesis can revise a byte that was already committed. Because
 * committed_text is append-only it is NOT rolled back: when full_text
 * diverges before the committed boundary, committed_text + tentative_text
 * no longer reconstruct full_text, so the committed/tentative seam is
 * transiently incoherent mid-stream. full_text is the authoritative raw
 * hypothesis at all times; committed_text trades that authority for a
 * flicker-free, append-only prefix. Consumers needing exact truth should
 * render full_text. Raising stable_prefix_agreement_n makes a wrong commit
 * less likely at the cost of committing later.
 *
 * On successful stream_finalize, tentative_text is empty. For ON_FINALIZE,
 * committed_text becomes final full_text. For policies that committed bytes
 * before finalize, finalize only appends compatible remaining suffix bytes;
 * if final full_text disagrees with already committed_text, committed_text
 * is not silently rewritten and full_text remains the authoritative raw
 * final transcript.
 *
 * All pointers are borrowed session-owned storage. They remain valid until
 * the next transcribe_stream_feed / transcribe_stream_finalize /
 * transcribe_stream_begin / transcribe_stream_reset / transcribe_run /
 * transcribe_session_free on the same session.
 */
struct transcribe_stream_text {
    uint64_t     struct_size;
    const char * full_text;
    uint64_t     full_text_bytes;
    const char * committed_text;
    uint64_t     committed_text_bytes;
    const char * tentative_text;
    uint64_t     tentative_text_bytes;
    uint64_t     raw_tentative_start_bytes;
};

TRANSCRIBE_API void transcribe_stream_text_init(struct transcribe_stream_text * out);

TRANSCRIBE_API transcribe_status transcribe_stream_get_text(const struct transcribe_session * session,
                                                            struct transcribe_stream_text *   out);

/*
 * Begin a streaming run on session.
 *
 * run_params and stream_params may be NULL for library defaults
 * (transcribe task, no timestamps, no family extension). To customize,
 * initialize via transcribe_run_params_init() / transcribe_stream_params_init()
 * and set fields. The "session==NULL is INVALID_ARG, struct_size==0 is
 * BAD_STRUCT_SIZE" rules from the top-of-header "Params" block apply.
 *
 * On success the session transitions IDLE/FINISHED/FAILED -> ACTIVE
 * and every result-visible field is cleared: text, segments, words,
 * tokens, detected language, stream revision, committed counts,
 * audio cursors, timings, was_aborted, and last stream status. The
 * installed abort callback, the loaded model, and any reusable
 * stream buffers held by the family are preserved.
 *
 * Returns:
 *   TRANSCRIBE_ERR_INVALID_ARG       session NULL, session in ACTIVE
 *                                    state, out-of-range enum in
 *                                    run_params, or an extension whose
 *                                    kind is unknown to / not accepted
 *                                    by the loaded model on the
 *                                    TRANSCRIBE_EXT_SLOT_STREAM slot.
 *   TRANSCRIBE_ERR_BAD_STRUCT_SIZE   non-null run_params or stream_params
 *                                    has a struct_size below what this
 *                                    entry point requires (including
 *                                    struct_size == 0), or
 *                                    stream_params->family is too small
 *                                    to contain a transcribe_ext header,
 *                                    or (for a family that implements the
 *                                    preflight, see below) a family
 *                                    extension whose size is below its
 *                                    kind's minimum. All of these are
 *                                    reported before the previous result
 *                                    snapshot is cleared.
 *                                    Use the init functions to avoid.
 *   TRANSCRIBE_ERR_NOT_IMPLEMENTED   Model has no streaming hooks or
 *                                    capabilities advertise
 *                                    supports_streaming == false.
 *                                    All three of stream_begin /
 *                                    stream_feed / stream_finalize must
 *                                    be wired for begin to succeed;
 *                                    stream_reset is optional.
 *   TRANSCRIBE_ERR_UNSUPPORTED_TASK  TRANSCRIBE_TASK_TRANSLATE; v1
 *                                    streaming is transcribe-only.
 *   TRANSCRIBE_ERR_UNSUPPORTED_TIMESTAMPS
 *                                    Requested granularity finer than
 *                                    the model's max_timestamp_kind.
 *   TRANSCRIBE_ERR_UNSUPPORTED_LANGUAGE
 *                                    run_params->language not in the
 *                                    model's declared language list.
 *
 * Failure semantics — three checkpoints, split by whether the previous
 * result snapshot survives:
 *
 *   1. Dispatcher preflight. Top-level argument checks run first:
 *      NULL session, ACTIVE-state rejection, struct_size failures,
 *      out-of-range run_params enums, unknown / unaccepted extension
 *      kind for the STREAM slot, TRANSLATE, granularity finer than the
 *      model's max, and unsupported language. On failure the call
 *      returns without entering ACTIVE and WITHOUT clearing the
 *      previous result snapshot. Lifecycle state is untouched.
 *
 *   2. Family preflight (optional, via the family's stream_validate
 *      hook). For families that implement it, family-specific value
 *      checks — e.g. a streaming extension field outside the model's
 *      trained menu, or below its kind's minimum size — run here, still
 *      BEFORE the snapshot is cleared and BEFORE the lifecycle moves.
 *      A non-OK return is delivered to the caller with the previous
 *      result snapshot fully preserved and no FAILED transition, so a
 *      caller-side config typo cannot destroy the prior utterance's
 *      transcript. A family that does not implement stream_validate
 *      performs these checks in stream_begin.
 *
 *   3. Post-clear stream_begin. Once the preflight checks pass, the dispatcher
 *      clears the result snapshot, enters ACTIVE, and calls the
 *      family's stream_begin hook. Any non-OK status from this point
 *      (config a non-preflighting family only catches here, allocation
 *      failure, etc.) transitions the stream to FAILED and preserves
 *      the status in transcribe_stream_last_status. The result snapshot
 *      in this case is whatever the hook wrote before failing —
 *      typically empty.
 *
 * Params lifetime: everything the caller passes is copied out before this
 * function returns. run_params strings (language / target_language) are
 * copied into session-owned storage, and family extensions follow their
 * documented copy-out contract — so the caller may free both params
 * structs and every pointer inside them the moment begin returns, while
 * the stream stays ACTIVE. Nothing handed to begin needs to outlive it.
 */
TRANSCRIBE_API transcribe_status transcribe_stream_begin(struct transcribe_session *             session,
                                                         const struct transcribe_run_params *    run_params,
                                                         const struct transcribe_stream_params * stream_params);

/*
 * Feed PCM into the active stream. 16 kHz mono float32, same as
 * transcribe_run.
 *
 * pcm must be non-null and n_samples must be strictly greater than
 * zero. Polling the stream without supplying audio is unsupported —
 * use the stream accessors (transcribe_stream_revision,
 * transcribe_stream_get_text, transcribe_stream_n_committed_*,
 * transcribe_stream_last_status, transcribe_stream_get_state) to inspect
 * state without progressing the stream.
 *
 * update is nullable; when non-null the dispatcher zero-initializes
 * it before calling the family hook, so callers may rely on a clean
 * struct even on early-return error paths.
 *
 * Returns TRANSCRIBE_ERR_INVALID_ARG when session is NULL, when state
 * is not ACTIVE, or on malformed input. A terminal non-OK status
 * from the family hook transitions the stream to FAILED and is
 * preserved in transcribe_stream_last_status. TRANSCRIBE_ERR_ABORTED
 * is a terminal status; transcribe_was_aborted distinguishes it from
 * other failures.
 */
TRANSCRIBE_API transcribe_status transcribe_stream_feed(struct transcribe_session *       session,
                                                        const float *                     pcm,
                                                        int                               n_samples,
                                                        struct transcribe_stream_update * update);

/*
 * Signal end of input. Flushes buffered audio, satisfies right-
 * context / lookahead requirements, and emits remaining text.
 *
 * On success the session transitions ACTIVE -> FINISHED. On a
 * terminal non-OK status the session transitions to FAILED and the
 * status is preserved in transcribe_stream_last_status.
 *
 * Returns TRANSCRIBE_ERR_INVALID_ARG when session is NULL or state is
 * not ACTIVE.
 */
TRANSCRIBE_API transcribe_status transcribe_stream_finalize(struct transcribe_session *       session,
                                                            struct transcribe_stream_update * update);

/*
 * Abandon the current stream without finalizing.
 *
 * Always returns the session to IDLE and clears every result-visible
 * field, every stream-snapshot counter, last_status, and the family's
 * per-utterance streaming state. Allocated stream buffers held by the
 * family are preserved for reuse — full memory release is
 * transcribe_session_free.
 *
 * Reset from IDLE clears any stale result/snapshot from a previous
 * stream or run. Reset from FINISHED or FAILED clears the surviving
 * result text and snapshot counters as well.
 *
 * No-op if session is NULL.
 */
TRANSCRIBE_API void transcribe_stream_reset(struct transcribe_session * session);

/*
 * Current stream lifecycle state. Returns TRANSCRIBE_STREAM_IDLE if
 * session is NULL.
 */
TRANSCRIBE_API enum transcribe_stream_state transcribe_stream_get_state(const struct transcribe_session * session);

/*
 * Monotonic snapshot revision counter. Advances whenever any observable
 * property of the streaming result changes: text vectors (tokens /
 * words / segments / full_text), committed-prefix counts, or a
 * lifecycle transition that promotes the snapshot (finalize moves
 * tentative output to committed even when the text is unchanged).
 * Reset to 0 by begin / reset / run. Returns 0 if session is NULL.
 *
 * Diff against the previous value when redrawing — a bump does not
 * by itself mean the visible text changed; it means "something about
 * the snapshot moved, re-read the accessors."
 */
TRANSCRIBE_API int transcribe_stream_revision(const struct transcribe_session * session);

/*
 * Low-level raw row boundary hints. In older streaming APIs these were
 * the primary committed-prefix interface. They remain useful for callers
 * that inspect raw token/word/segment rows, but the strong API-stable
 * display contract is now transcribe_stream_get_text(). A committed text
 * prefix may be independent from the current raw rows after the model
 * revises already-committed bytes.
 *
 * Return 0 if session is NULL.
 */
TRANSCRIBE_API int transcribe_stream_n_committed_segments(const struct transcribe_session * session);
TRANSCRIBE_API int transcribe_stream_n_committed_words(const struct transcribe_session * session);
TRANSCRIBE_API int transcribe_stream_n_committed_tokens(const struct transcribe_session * session);

/*
 * Last terminal status of the stream. Preserves the failing status
 * after a feed/finalize call transitioned the stream to FAILED, so
 * the caller can inspect it after the fact. Reset to TRANSCRIBE_OK
 * by begin / reset / run.
 *
 * Returns TRANSCRIBE_OK if session is NULL or no terminal status has
 * been recorded on the current stream.
 */
TRANSCRIBE_API transcribe_status transcribe_stream_last_status(const struct transcribe_session * session);

/* ----------------------------------------------------------------------- */
/* Tokenization                                                            */
/* ----------------------------------------------------------------------- */

/*
 * Tokenize plain UTF-8 text into the model's vocabulary, with
 * add_special_tokens=False semantics (no BOS/EOS, no <|...|> markers).
 * Special tokens present in the input are not recognized and will be
 * BPE-encoded piece-by-piece, which is almost never what the caller
 * wants — render special tokens via direct id paths and pass only the
 * plain-text fragments through this function.
 *
 * Buffer contract (mirrors whisper.cpp's n_max convention):
 *
 *   >= 0             Number of tokens written to `tokens[0..return-1]`.
 *   negative of N    Buffer too small; N tokens were needed. Caller
 *                    reallocates and retries. `tokens` content is
 *                    unspecified in this case (may be partially
 *                    written).
 *   INT_MIN          Hard error: model or text is NULL, the model's
 *                    tokenizer does not support encode for this
 *                    vocabulary (e.g. a SentencePiece family without
 *                    encode wired), or the vocab file is malformed.
 *
 * Plumbed per family:
 *   - Whisper      (GPT-2 byte-level BPE)
 *   - Qwen3-ASR    (Qwen2 byte-level BPE)
 *   - Parakeet     (SentencePiece; currently returns INT_MIN)
 *   - Cohere ASR   (SentencePiece; currently returns INT_MIN)
 */
TRANSCRIBE_API int transcribe_tokenize(const struct transcribe_model * model,
                                       const char *                    text,
                                       int32_t *                       tokens,
                                       size_t                          n_max);

/* ----------------------------------------------------------------------- */
/* Timings                                                                 */
/* ----------------------------------------------------------------------- */

/*
 * Per-call timings collected by the most recent transcribe_run on a
 * session, plus the wall-clock load time of the model the session is
 * bound to. All values are in milliseconds.
 *
 *   load_ms    one-time wall-clock cost of transcribe_model_load_file,
 *              captured on the model and surfaced via every session
 *              derived from it. Not affected by transcribe_reset_timings
 *              (it's a model-scoped fact).
 *   mel_ms     time to compute the mel front-end on the most recent
 *              transcribe_run. Reset to 0 by transcribe_reset_timings.
 *   encode_ms  time to run the encoder forward pass on the most recent
 *              transcribe_run.
 *   decode_ms  time to run the decoder (predictor + joint + token
 *              search) on the most recent transcribe_run.
 *
 * Output struct: caller initializes via transcribe_timings_init() (zero-
 * fill); the library writes only the fields that fit within
 * struct_size. Every field is a numeric value where zero encodes
 * "unknown / not measured."
 */
struct transcribe_timings {
    uint64_t struct_size;
    float    load_ms;
    float    mel_ms;
    float    encode_ms;
    float    decode_ms;
};

TRANSCRIBE_API void transcribe_timings_init(struct transcribe_timings * out);

/*
 * Read the current timings from a session into caller-owned storage.
 * Safe to call before any transcribe_run; mel_ms / encode_ms /
 * decode_ms will be 0. load_ms is non-zero as soon as the underlying
 * model is loaded.
 *
 * Returns:
 *   TRANSCRIBE_ERR_INVALID_ARG     session or out_timings is NULL.
 *   TRANSCRIBE_ERR_BAD_STRUCT_SIZE out_timings->struct_size is 0 or
 *                                  smaller than the library's minimum.
 */
TRANSCRIBE_API transcribe_status transcribe_get_timings(const struct transcribe_session * session,
                                                        struct transcribe_timings *       out_timings);

/*
 * Pretty-print the current timings to the registered log callback at
 * INFO level (or stderr if no callback is installed). No-op if session
 * is NULL.
 */
TRANSCRIBE_API void transcribe_print_timings(const struct transcribe_session * session);

/*
 * Reset the per-run timing accumulators (mel_ms, encode_ms,
 * decode_ms) to 0. Does NOT touch load_ms — that's a model-scoped
 * fact. No-op if session is NULL.
 */
TRANSCRIBE_API void transcribe_reset_timings(struct transcribe_session * session);

/* ----------------------------------------------------------------------- */
/* Result accessors - top level                                            */
/* ----------------------------------------------------------------------- */

TRANSCRIBE_API const char *              transcribe_full_text(const struct transcribe_session * session);
TRANSCRIBE_API transcribe_timestamp_kind transcribe_returned_timestamp_kind(const struct transcribe_session * session);
TRANSCRIBE_API int                       transcribe_n_segments(const struct transcribe_session * session);
TRANSCRIBE_API int                       transcribe_n_words(const struct transcribe_session * session);
TRANSCRIBE_API int                       transcribe_n_tokens(const struct transcribe_session * session);

/*
 * The language the model itself predicted on the most recent run, as a
 * short ISO code ("en", "zh", "yue", "ja", "ko", ...). Returns an empty
 * string when:
 *   - no successful run has happened on this session yet, or
 *   - the caller passed an explicit `params->language` hint (the
 *     library does not echo hints back through this field; callers
 *     already know what they asked for), or
 *   - the model does not support language detection (English-only
 *     Whispers, families without an LID head), or
 *   - the family's LID head produced a non-language sentinel for this
 *     audio (e.g. SenseVoice's <|nospeech|>).
 *
 * Returned pointer is owned by the session. See "Result text-pointer
 * lifetime" at the top of this header for invalidation rules; in
 * streaming mode every transcribe_stream_feed / _finalize call may
 * invalidate it.
 */
TRANSCRIBE_API const char * transcribe_detected_language(const struct transcribe_session * session);

/* ----------------------------------------------------------------------- */
/* Result accessors - per-item rows                                        */
/* ----------------------------------------------------------------------- */

/*
 * Per-item results are exposed as caller-owned copy-out structs. Three
 * accessors (transcribe_get_segment / _word / _token) take a row index
 * and a pointer to an init-function-initialized struct; the library
 * writes the row's fields into the caller's buffer.
 *
 * `text` pointers in these structs are session-owned. The full lifetime
 * contract is in the "Result text-pointer lifetime" block at the top of
 * this header; in short:
 *   - One-shot path: valid until the next transcribe_run() /
 *     transcribe_stream_begin() / transcribe_stream_reset() /
 *     transcribe_session_free() call on the same session.
 *   - Streaming path: row `text` aliases the raw model snapshot and may
 *     be invalidated by every transcribe_stream_feed() /
 *     transcribe_stream_finalize() call. Copy it before the next feed if
 *     you need to hold it. Use transcribe_stream_get_text() for
 *     API-stable committed/provisional display text.
 *
 * Out-of-range index (i < 0 or i >= the corresponding transcribe_n_*())
 * returns TRANSCRIBE_OK with the caller's struct left as zero-init:
 * `text == NULL`, every scalar 0, and (for tokens) `p == 0.0f`. There
 * is no empty-string sentinel — bindings distinguish "row not present"
 * from "row present with empty text" by null-checking `text`.
 *
 * Forward-ABI: struct_size in each row struct is captured at the
 * caller's TU; the library writes only the fields that fit and never
 * touches tail bytes beyond the caller's struct_size. New fields are
 * appended at the end.
 */

struct transcribe_segment {
    uint64_t     struct_size;
    int64_t      t0_ms;
    int64_t      t1_ms;
    int          first_word;
    int          n_words;
    int          first_token;
    int          n_tokens;
    const char * text; /* session-owned; see lifetime note above */
};

TRANSCRIBE_API void transcribe_segment_init(struct transcribe_segment * out);

struct transcribe_word {
    uint64_t     struct_size;
    int64_t      t0_ms;
    int64_t      t1_ms;
    int          seg_index;
    int          first_token;
    int          n_tokens;
    const char * text; /* session-owned; see lifetime note above */
};

TRANSCRIBE_API void transcribe_word_init(struct transcribe_word * out);

/*
 * transcribe_token::p is the per-token probability when the
 * architecture produces one, or NaN when it does not. The semantic of
 * "token probability" varies by family (joint softmax for transducer,
 * per-step softmax for autoregressive, per-frame argmax probability
 * for CTC) and callers should treat it as a confidence hint, not a
 * calibrated probability.
 *
 * On out-of-range index `p` follows the zero-init rule (0.0f, not
 * NaN); inspect `text != NULL` to distinguish a present row.
 */
struct transcribe_token {
    uint64_t     struct_size;
    int          id;
    float        p;
    int64_t      t0_ms;
    int64_t      t1_ms;
    int          seg_index;
    int          word_index;
    const char * text; /* session-owned; see lifetime note above */
};

TRANSCRIBE_API void transcribe_token_init(struct transcribe_token * out);

/*
 * Read one segment / word / token row into caller-owned storage.
 *
 * Returns:
 *   TRANSCRIBE_ERR_INVALID_ARG     out is NULL.
 *   TRANSCRIBE_ERR_BAD_STRUCT_SIZE out->struct_size is 0 or smaller
 *                                  than the library's minimum.
 *   TRANSCRIBE_OK                  otherwise. The caller's struct is
 *                                  written when session is non-NULL, the
 *                                  session has a result, and i is in
 *                                  range; otherwise it stays as
 *                                  zero-initialized by INIT (text NULL,
 *                                  scalars 0).
 */
TRANSCRIBE_API transcribe_status transcribe_get_segment(const struct transcribe_session * session,
                                                        int                               i,
                                                        struct transcribe_segment *       out);

TRANSCRIBE_API transcribe_status transcribe_get_word(const struct transcribe_session * session,
                                                     int                               i,
                                                     struct transcribe_word *          out);

TRANSCRIBE_API transcribe_status transcribe_get_token(const struct transcribe_session * session,
                                                      int                               i,
                                                      struct transcribe_token *         out);

/* ----------------------------------------------------------------------- */
/* Batch result accessors                                                  */
/* ----------------------------------------------------------------------- */

/*
 * Read back the per-utterance results produced by transcribe_run_batch.
 * These mirror the single-result accessors above with a leading utterance
 * index `i` in [0, transcribe_batch_n_results(session)).
 *
 * After a successful transcribe_run, n_results is 1 and index 0 is the
 * single result, so callers may use either accessor family interchangeably
 * for the n == 1 case. After transcribe_run_batch, index 0 also aliases
 * the legacy single-result accessors.
 *
 * Lifetime and out-of-range rules match the single-result accessors:
 *   - Returned `const char *` and row `text` pointers are session-owned
 *     and valid until the next transcribe_run / transcribe_run_batch /
 *     transcribe_stream_begin / transcribe_stream_reset /
 *     transcribe_session_free on the same session.
 *   - An out-of-range utterance index `i`, or an out-of-range row index
 *     `j`, yields a safe sentinel: "" / 0 / NaN for scalar returns, and a
 *     zero-initialized struct (text == NULL) for the copy-out accessors,
 *     which still return TRANSCRIBE_OK (struct-size faults and a NULL
 *     `out` return their usual non-OK codes).
 *   - Two accessors are deliberate exceptions to the sentinel rule and
 *     instead return TRANSCRIBE_ERR_INVALID_ARG for an out-of-range (or
 *     negative) utterance index `i`: transcribe_batch_status (TRANSCRIBE_OK
 *     is reserved for a real per-utterance success, so it cannot also
 *     sentinel a missing utterance) and transcribe_batch_get_timings
 *     (timings are utterance-scoped, not a row accessor like
 *     transcribe_batch_get_segment / _word / _token, so a missing utterance
 *     is an error rather than an empty row). The per-function docs below are
 *     authoritative where they differ from this summary.
 */

/*
 * Number of per-utterance results available. 0 before any run, or if
 * session is NULL. 1 after a successful transcribe_run. n after a
 * transcribe_run_batch with n utterances that returns OK or
 * TRANSCRIBE_ERR_ABORTED — one slot per input utterance, including
 * utterances that failed individually (non-OK transcribe_batch_status,
 * empty rows) and, after an abort, utterances not retained as completed
 * results (present as TRANSCRIBE_ERR_ABORTED slots). After a whole-batch
 * fault other than abort (OOM, backend error) the count is unspecified;
 * the non-OK top-level return is the signal in that case.
 */
TRANSCRIBE_API int transcribe_batch_n_results(const struct transcribe_session * session);

/*
 * Per-utterance terminal status. TRANSCRIBE_OK when utterance i produced a
 * result; otherwise the status that failed that utterance (e.g.
 * TRANSCRIBE_ERR_INVALID_ARG for a malformed input, or a backend error).
 * Returns TRANSCRIBE_ERR_INVALID_ARG if session is NULL or i is out of
 * range.
 */
TRANSCRIBE_API transcribe_status transcribe_batch_status(const struct transcribe_session * session, int i);

TRANSCRIBE_API const char * transcribe_batch_full_text(const struct transcribe_session * session, int i);

TRANSCRIBE_API transcribe_timestamp_kind
transcribe_batch_returned_timestamp_kind(const struct transcribe_session * session, int i);

TRANSCRIBE_API const char * transcribe_batch_detected_language(const struct transcribe_session * session, int i);

TRANSCRIBE_API int transcribe_batch_n_segments(const struct transcribe_session * session, int i);

TRANSCRIBE_API int transcribe_batch_n_words(const struct transcribe_session * session, int i);

TRANSCRIBE_API int transcribe_batch_n_tokens(const struct transcribe_session * session, int i);

/*
 * Copy out row `j` of utterance `i`. The struct is initialized exactly as
 * for transcribe_get_segment / _word / _token (caller calls the matching
 * _init first); the only difference is the utterance index `i`.
 */
TRANSCRIBE_API transcribe_status transcribe_batch_get_segment(const struct transcribe_session * session,
                                                              int                               i,
                                                              int                               j,
                                                              struct transcribe_segment *       out);

TRANSCRIBE_API transcribe_status transcribe_batch_get_word(const struct transcribe_session * session,
                                                           int                               i,
                                                           int                               j,
                                                           struct transcribe_word *          out);

TRANSCRIBE_API transcribe_status transcribe_batch_get_token(const struct transcribe_session * session,
                                                            int                               i,
                                                            int                               j,
                                                            struct transcribe_token *         out);

/*
 * Per-utterance timings for a batched run. Mirrors transcribe_get_timings but
 * indexed by utterance. load_ms is the model-scoped load time (same for every
 * utterance). mel_ms / encode_ms / decode_ms are this utterance's stage times;
 * for a batched encoder the encode_ms is the shared batch encode time
 * amortized across the batch (so the sum over utterances equals the real batch
 * encode time), while decode_ms is the genuine per-utterance host-decode cost.
 * This is what lets a caller see how run time splits between the GPU encoder
 * and the host decoder as batch size grows.
 *
 * Returns TRANSCRIBE_ERR_INVALID_ARG on NULL args or out-of-range i,
 * TRANSCRIBE_ERR_BAD_STRUCT_SIZE on an uninitialized out struct.
 */
TRANSCRIBE_API transcribe_status transcribe_batch_get_timings(const struct transcribe_session * session,
                                                              int                               i,
                                                              struct transcribe_timings *       out);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* TRANSCRIBE_H */
