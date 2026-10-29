/*
 * include/transcribe/whisper.h - Whisper-family public extension surface.
 *
 * Includes transcribe.h; safe to include in C or C++ TUs. Holds:
 *   - the Whisper-family run extension struct (initial prompt + token
 *     conditioning, temperature fallback tuple, no-speech gate);
 *   - the family telemetry chunk-trace struct;
 *   - the disabled-threshold sentinel macros (+/-INF) and the prompt
 *     composition enum used by the run ext.
 *
 * Whisper exposes substantial real model-specific knobs (13 fields).
 * The PNC/ITN toggles that other families share via transcribe_run_params
 * do not apply: transcribe_model_supports(model, TRANSCRIBE_FEATURE_PNC)
 * and (..., TRANSCRIBE_FEATURE_ITN) both return false for whisper, and a
 * non-DEFAULT pnc/itn against a whisper model produces a WARN.
 * Probe via transcribe_model_accepts_ext_kind before pointing
 * transcribe_run_params::family at this struct.
 *
 * FourCC kinds are reserved in docs/extension-kinds.md.
 */

#ifndef TRANSCRIBE_WHISPER_H
#define TRANSCRIBE_WHISPER_H

#include "transcribe.h"

#include <math.h> /* INFINITY (MSVC rejects a constant 1.0/0.0 — error C2124) */

#ifdef __cplusplus
extern "C" {
#endif

/* ----------------------------------------------------------------------- */
/* Whisper run extension                                                   */
/* ----------------------------------------------------------------------- */

/* 'WHRN' little-endian = 0x4E524857 */
#define TRANSCRIBE_EXT_KIND_WHISPER_RUN 0x4E524857u

/*
 * Sentinels for "disabled" on threshold fields. Never rely on 0.0 as a
 * sentinel — it is a legitimate value for logprob_thold and lies in
 * the normal range of the other thresholds.
 *
 *   Check shape "X > thold"  →  disable with +INF (_THOLD_DISABLED).
 *   Check shape "X < thold"  →  disable with -INF (_LOGPROB_DISABLED).
 */
#define TRANSCRIBE_WHISPER_THOLD_DISABLED   ((float) INFINITY)  /* +INF */
#define TRANSCRIBE_WHISPER_LOGPROB_DISABLED ((float) -INFINITY) /* -INF */

/*
 * How an initial prompt composes with condition_on_prev_tokens on
 * long-form runs. Matches HF's prompt_condition_type string knob.
 *
 *   FIRST_SEGMENT (default): the initial prompt sits at the head of
 *     the first chunk's prefix. If condition_on_prev_tokens is true,
 *     subsequent chunks carry the decoded prior-chunk tokens under
 *     <|startofprev|> and the initial prompt is visible only while
 *     it still fits in the sliding 223-token window.
 *
 *   ALL_SEGMENTS: the initial prompt replaces <|startofprev|> as the
 *     persistent BOS of the prev-context window on EVERY chunk.
 *     Requires condition_on_prev_tokens=true; passing this mode
 *     without condition_on_prev_tokens returns
 *     TRANSCRIBE_ERR_INVALID_ARG (mirrors HF's ValueError).
 */
enum transcribe_whisper_prompt_condition {
    TRANSCRIBE_WHISPER_PROMPT_FIRST_SEGMENT = 0,
    TRANSCRIBE_WHISPER_PROMPT_ALL_SEGMENTS  = 1,
};

/*
 * Whisper-family run knobs. Reached via transcribe_run_params::family.
 * NULL family selects transcribe_whisper_run_ext_init() values (Whisper's
 * own shipping recipe: temperature fallback on, compression-ratio /
 * avg-logprob / no-speech safety nets active). Callers that want HF
 * library-wide generate() behavior ("all thresholds disabled") set each
 * _thold field to its _DISABLED sentinel.
 *
 * Pointer lifetime: the library copies referenced data (initial_prompt
 * bytes, prompt_tokens contents) into context state before
 * transcribe_run returns. The caller may free both buffers immediately
 * after the call.
 */
struct transcribe_whisper_run_ext {
    struct transcribe_ext ext;

    /*
     * Initial prompt / prior context.
     *
     *   If prompt_tokens != NULL:
     *       Use prompt_tokens verbatim (caller owns the bytes; they must
     *       NOT include the <|startofprev|> marker — the library prepends
     *       it). initial_prompt is IGNORED.
     *
     *   Else if initial_prompt != NULL:
     *       Tokenize as HF's get_prompt_ids does:
     *           "<|startofprev|>" + " " + initial_prompt.strip()
     *       The leading space is mandatory (matches
     *       transformers tokenization_whisper.py:710-722). Any special
     *       token (<|...|>) found in the tokenized prompt text is
     *       rejected with TRANSCRIBE_ERR_INVALID_ARG, mirroring HF's
     *       own check.
     *
     *   Else: no initial prompt.
     */
    const char *    initial_prompt;
    const int32_t * prompt_tokens;
    size_t          n_prompt_tokens;

    /* See transcribe_whisper_prompt_condition above. Default FIRST_SEGMENT. */
    enum transcribe_whisper_prompt_condition prompt_condition;

    /*
     * Per HF generation_whisper.py:1853-1918. When true and any prior
     * chunk decoded successfully at temperature < 0.5, the tail of the
     * prior chunk's tokens is prepended (under <|startofprev|>) to the
     * next chunk's prefix, capped at max_prev_context_tokens. Auto-
     * disables for the next chunk when the prior chunk was accepted at
     * temperature >= 0.5 (matches HF ":1090-1093").
     */
    bool condition_on_prev_tokens;

    /* Cap for carried prev tokens; default 223 = max_target_positions/2 - 1. */
    int32_t max_prev_context_tokens;

    /*
     * Sampling + temperature fallback. Default behavior matches
     * Whisper's own recipe, not HF generate()'s library-default.
     * Use the _DISABLED sentinels to turn off individual thresholds.
     */
    float temperature;             /* first-tier; default 0.0 */
    float temperature_inc;         /* default 0.2             */
    float compression_ratio_thold; /* default 2.4             */
    float logprob_thold;           /* default -1.0            */
    float no_speech_thold;         /* default 0.6             */

    /*
     * Seed for the sampler at temperature > 0. 0 = nondeterministic
     * (matches the whisper.cpp convention). A nonzero seed produces
     * reproducible output across runs at matching temperatures.
     * Ignored when temperature == 0.0 (greedy decode is deterministic
     * by construction).
     */
    uint32_t seed;

    /* Seconds. Caps the first emitted timestamp; default 1.0. */
    float max_initial_timestamp;
};

/* Fills ext.size/kind and the Whisper decoding recipe defaults. */
TRANSCRIBE_API void transcribe_whisper_run_ext_init(struct transcribe_whisper_run_ext * ext);

/* ----------------------------------------------------------------------- */
/* Whisper decoding trace                                                  */
/* ----------------------------------------------------------------------- */

/*
 * Per-chunk observability: the temperature tier that the fallback loop
 * accepted, the metric values that drove acceptance, and whether the
 * no-speech gate fired. Populated on every successful transcribe_run()
 * call for a Whisper context; one entry per 30-second encoder window
 * that actually decoded (no-speech-skipped chunks still emit an entry).
 *
 * The chunk window `t0_ms .. t1_ms` is the global encoder slice, not a
 * segment boundary. Segments carried in transcribe_n_segments() /
 * transcribe_segment_*() live inside these windows.
 *
 * Output struct: caller initializes via transcribe_whisper_chunk_trace_init()
 * (zero-fill). The library writes only fields that fit within struct_size
 * and never touches tail bytes beyond it; every field is designed so the
 * zero value means "absent / unknown / false."
 */
struct transcribe_whisper_chunk_trace {
    uint64_t struct_size;
    int64_t  t0_ms;
    int64_t  t1_ms;
    float    temperature_used; /* the tier that was accepted */
    float    compression_ratio;
    float    avg_logprob;
    float    no_speech_prob;
    bool     no_speech_triggered; /* chunk output was discarded */
    int32_t  n_fallbacks;         /* tiers tried before accept (0 = tier 0 accepted) */
};

TRANSCRIBE_API void transcribe_whisper_chunk_trace_init(struct transcribe_whisper_chunk_trace * out);

/*
 * Number of chunk traces captured on the most recent successful run.
 * Returns 0 before any run or on non-Whisper contexts.
 */
TRANSCRIBE_API int transcribe_get_whisper_chunk_count(const struct transcribe_session * session);

/*
 * Read the trace at index [0, count) into caller-owned storage.
 * Out-of-range indices and non-Whisper contexts succeed and leave the
 * caller's struct as initialized (zero-filled).
 *
 * Returns:
 *   TRANSCRIBE_ERR_INVALID_ARG     out_trace is NULL.
 *   TRANSCRIBE_ERR_BAD_STRUCT_SIZE out_trace->struct_size is 0 or
 *                                  smaller than the library's minimum.
 *   TRANSCRIBE_OK                  otherwise. The caller's struct is
 *                                  written when session is a Whisper context
 *                                  and i is in range; otherwise it is
 *                                  left as zero-initialized by the init
 *                                  function.
 */
TRANSCRIBE_API transcribe_status transcribe_get_whisper_chunk_trace(const struct transcribe_session *       session,
                                                                    int                                     i,
                                                                    struct transcribe_whisper_chunk_trace * out_trace);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* TRANSCRIBE_WHISPER_H */
