/*
 * include/transcribe/langid.h - LANGID role: which language is spoken.
 *
 * Includes transcribe.h; safe to include in C or C++ TUs. For models whose
 * product is a language decision (transcribe_model_roles() has
 * TRANSCRIBE_ROLE_LANGID).
 *
 * Open a transcribe_langid_session on a loaded model, run it on a clip,
 * then read the ranked candidates. Labels are the model's own codes ("en",
 * "iw", "jw"); there is no canonicalization, so match a code against an ASR
 * model's transcribe_capabilities::languages yourself. Usage guide:
 * docs/langid.md. Threading and lifetime rules: docs/roles.md.
 */

#ifndef TRANSCRIBE_LANGID_H
#define TRANSCRIBE_LANGID_H

#include "transcribe.h"

#ifdef __cplusplus
extern "C" {
#endif

struct transcribe_langid_session;

/* Static facts about a language ID model. */
struct transcribe_langid_info {
    uint64_t struct_size;
    int32_t  sample_rate;  /* input PCM rate (16000) */
    int32_t  n_labels;     /* label indices are [0, n_labels) */
    int32_t  min_audio_ms; /* shorter scored audio is TRANSCRIBE_ERR_INPUT_TOO_SHORT */
    int32_t  max_audio_ms; /* longer input is scored on its FIRST max_audio_ms (30000) */
};

struct transcribe_langid_session_params {
    uint64_t struct_size;
    int32_t  n_threads; /* 0 = library default */
};

struct transcribe_langid_params {
    uint64_t             struct_size;
    /* Restrict the decision to these labels (codes or aliases, borrowed for
     * the call). NULL with n_allowed == 0 means every label; that is the
     * only "all" form. A non-NULL list with n_allowed <= 0, a NULL list
     * with n_allowed > 0, or a NULL element is INVALID_ARG. An unknown code
     * is TRANSCRIBE_ERR_UNSUPPORTED_LANGUAGE. Duplicates count once. */
    const char * const * allowed;
    int32_t              n_allowed;
};

/* Summary of the last run. */
struct transcribe_langid_result {
    uint64_t struct_size;
    /* Rows readable via transcribe_langid_get_candidate: one per label in the
     * allowed set (duplicates counted once), n_labels when unrestricted. */
    int32_t  n_candidates;
    /* Share of the softmax over every label that falls in the allowed set;
     * 1 when unrestricted. A low value means the speech is probably outside
     * the allowed set. */
    float    allowed_mass;
};

/* One ranked label. code and name point at model-owned storage, valid until
 * transcribe_model_free. */
struct transcribe_langid_candidate {
    uint64_t     struct_size;
    int32_t      index; /* label index */
    const char * code;
    const char * name;
    float        p; /* softmax over the allowed set */
    float        logit;
};

TRANSCRIBE_API void transcribe_langid_info_init(struct transcribe_langid_info * out);
TRANSCRIBE_API void transcribe_langid_session_params_init(struct transcribe_langid_session_params * params);
TRANSCRIBE_API void transcribe_langid_params_init(struct transcribe_langid_params * params);
TRANSCRIBE_API void transcribe_langid_result_init(struct transcribe_langid_result * out);
TRANSCRIBE_API void transcribe_langid_candidate_init(struct transcribe_langid_candidate * out);

/* UNSUPPORTED_ROLE when the model does not serve TRANSCRIBE_ROLE_LANGID. */
TRANSCRIBE_API transcribe_status transcribe_langid_get_info(const struct transcribe_model * model,
                                                            struct transcribe_langid_info * out);

/* The label table. Code and name of label i, or NULL when i is out of range
 * or the model does not serve TRANSCRIBE_ROLE_LANGID; model-owned. */
TRANSCRIBE_API const char * transcribe_langid_label_code(const struct transcribe_model * model, int32_t i);
TRANSCRIBE_API const char * transcribe_langid_label_name(const struct transcribe_model * model, int32_t i);
/* Index of a code or alias ("he" and "iw" name the same label), or -1. */
TRANSCRIBE_API int32_t transcribe_langid_label_index(const struct transcribe_model * model, const char * code_or_alias);

/* params may be NULL for defaults. UNSUPPORTED_ROLE when the model does not
 * serve TRANSCRIBE_ROLE_LANGID. On failure *out_session is NULL. */
TRANSCRIBE_API transcribe_status transcribe_langid_session_init(struct transcribe_model *                       model,
                                                                const struct transcribe_langid_session_params * params,
                                                                struct transcribe_langid_session ** out_session);

/* NULL is a no-op. */
TRANSCRIBE_API void transcribe_langid_session_free(struct transcribe_langid_session * session);

/* Polled during a run; returning true stops it with TRANSCRIBE_ERR_ABORTED. */
TRANSCRIBE_API void transcribe_langid_set_abort_callback(struct transcribe_langid_session * session,
                                                         transcribe_abort_callback          cb,
                                                         void *                             user_data);

/*
 * Identify the language of one clip: 16 kHz mono float32 PCM, every sample
 * finite. params may be NULL for defaults. Input longer than
 * transcribe_langid_info::max_audio_ms is scored on its first max_audio_ms
 * (no error). Every allowed label is ranked. Replaces the previous result.
 * Malformed input (NULL pointers, n_samples <= 0, NaN / Inf, a bad allowed
 * list, an unknown code, scored audio shorter than min_audio_ms) returns an
 * error before the previous result is touched.
 */
TRANSCRIBE_API transcribe_status transcribe_langid_run(struct transcribe_langid_session *      session,
                                                       const float *                           pcm,
                                                       int                                     n_samples,
                                                       const struct transcribe_langid_params * params);

/*
 * The last run's summary. Zeroed before any run and after a run that passed
 * input validation but then failed (aborted, backend error).
 */
TRANSCRIBE_API transcribe_status transcribe_langid_get_result(const struct transcribe_langid_session * session,
                                                              struct transcribe_langid_result *        out);

/* Candidate i of the last run, ranked by p (descending; ties keep label
 * order). An out-of-range index returns OK with a zeroed row (code NULL). */
TRANSCRIBE_API transcribe_status transcribe_langid_get_candidate(const struct transcribe_langid_session * session,
                                                                 int                                      i,
                                                                 struct transcribe_langid_candidate *     out);

/* load_ms plus the last run's stage times (mel_ms, encode_ms). */
TRANSCRIBE_API transcribe_status transcribe_langid_get_timings(const struct transcribe_langid_session * session,
                                                               struct transcribe_timings *              out);

#ifdef __cplusplus
}
#endif

#endif /* TRANSCRIBE_LANGID_H */
