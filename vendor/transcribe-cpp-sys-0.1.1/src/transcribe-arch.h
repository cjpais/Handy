// transcribe-arch.h - per-family architecture trait + registry.
//
// INTERNAL. The public C ABI knows nothing about Arch; it dispatches
// through find_arch(). Each family provides exactly one transcribe::Arch
// instance whose function pointers implement its load / init_context / run
// behavior; the registry is an explicit array in transcribe-arch.cpp. The
// trait holds only what genuinely varies per family: teardown is handled by
// the base virtual destructors and introspection reads the base struct.

#pragma once

#include "transcribe.h"

namespace transcribe {

class Loader;

// Per-family trait. Function pointers may be null when an entry point is
// not yet implemented; the central dispatch converts null entries into
// TRANSCRIBE_ERR_NOT_IMPLEMENTED.
//
// Ownership conventions:
//   load / init_context: on success *out_model / *out_ctx points at a
//     heap-allocated derived object; on failure it is left null and a
//     non-OK status returned. load may call loader.release_gguf() to take
//     ownership of the gguf_context (and then free it, typically in the
//     model destructor); otherwise the Loader frees it on unwinding.
//   Cleanup is NOT in the trait: transcribe_model_free /
//     transcribe_session_free `delete` against the base virtual destructor.
struct Arch {
    // The string compared against general.architecture in the GGUF KV.
    // Must be a NUL-terminated string with static lifetime.
    const char * name;

    transcribe_status (*load)(Loader &                             loader,
                              const transcribe_model_load_params * params,
                              struct transcribe_model **           out_model);

    transcribe_status (*init_context)(struct transcribe_model *         model,
                                      const transcribe_session_params * params,
                                      struct transcribe_session **      out_ctx);

    transcribe_status (*run)(struct transcribe_session *   ctx,
                             const float *                 pcm,
                             int                           n_samples,
                             const transcribe_run_params * params);

    // Optional offline batch fast path. When non-null, transcribe_run_batch
    // delegates here to process all `n` utterances in one dispatch; when
    // null the dispatcher falls back to calling run() once per utterance,
    // so every family supports the batch API regardless.
    //
    // Contract:
    //   - The dispatcher has validated the shared run_params ONCE and
    //     cleared the scratch slot and ctx->batch_results.
    //   - pcm[i] / n_samples[i] are the i-th utterance (16 kHz mono f32).
    //     The hook does per-utterance input validation (a bad utterance is
    //     a non-OK ResultSet with has_result == false, not a batch error).
    //   - On OK the hook MUST push exactly `n` entries onto batch_results
    //     in order and SHOULD leave the scratch slot mirroring
    //     batch_results[0]. Poll ctx->poll_abort() between utterances; on
    //     abort, push the completed results and return TRANSCRIBE_ERR_ABORTED
    //     (the dispatcher pads batch_results back up to `n`).
    //   - The return value is the whole-batch status: OK when the dispatch
    //     ran (per-utterance failures live in each ResultSet), non-OK only
    //     for whole-batch faults (OOM, abort, backend error).
    transcribe_status (*run_batch)(struct transcribe_session *   ctx,
                                   const float * const *         pcm,
                                   const int *                   n_samples,
                                   int                           n,
                                   const transcribe_run_params * params);

    // Optional streaming hooks. stream_begin / stream_feed /
    // stream_finalize are a required triple (the dispatcher returns
    // NOT_IMPLEMENTED if any is NULL); stream_reset is optional (when NULL
    // the dispatcher still wipes its own state and runs clear_result).
    //
    //   stream_validate (optional): pure pre-flight validation of family
    //     params / stream_params extension, called BEFORE the result
    //     snapshot is cleared and the lifecycle moves. On non-OK the prior
    //     snapshot and lifecycle are fully preserved (no FAILED). The right
    //     place to vet caller input so a typo can't destroy a transcript.
    //   stream_begin: install per-utterance state. The dispatcher has
    //     already run stream_validate, cleared the snapshot, and set state
    //     to ACTIVE; on non-OK it transitions to FAILED and records the
    //     status in stream_last_status.
    //   stream_feed: consume the PCM (n_samples may be 0), update result
    //     vectors / committed counts / cursors in place, fill `update` when
    //     non-null. Poll ctx->poll_abort(); on abort return
    //     TRANSCRIBE_ERR_ABORTED.
    //   stream_finalize: flush buffered audio, emit remaining text, mark
    //     all output committed. Dispatcher -> FINISHED on OK, FAILED else.
    //   stream_reset: release per-utterance state / buffered audio (keeping
    //     allocations); the dispatcher forces state back to IDLE around it.
    transcribe_status (*stream_validate)(const struct transcribe_session * ctx,
                                         const transcribe_run_params *     run_params,
                                         const transcribe_stream_params *  stream_params);

    transcribe_status (*stream_begin)(struct transcribe_session *      ctx,
                                      const transcribe_run_params *    run_params,
                                      const transcribe_stream_params * stream_params);

    transcribe_status (*stream_feed)(struct transcribe_session * ctx,
                                     const float *               pcm,
                                     int                         n_samples,
                                     transcribe_stream_update *  update);

    transcribe_status (*stream_finalize)(struct transcribe_session * ctx, transcribe_stream_update * update);

    void (*stream_reset)(struct transcribe_session * ctx);

    // Optional kind+slot probe. Returns true when the loaded model variant
    // accepts the named extension kind on the given slot (run_params::family
    // for _RUN, stream_params::family for _STREAM). Acceptance is
    // per-loaded-variant AND per-slot. NULL is reserved for "no extension
    // surface at all" — a family with no surface for one slot returns false
    // rather than NULL'ing the hook.
    bool (*accepts_ext_kind)(const struct transcribe_model * model, transcribe_ext_slot slot, uint32_t kind);

    // Optional pre-clear validation for the one-shot transcribe_run path,
    // the _RUN-slot analogue of stream_validate. Called as the LAST gate
    // before clear_result wipes the snapshot, after the dispatcher's own
    // pre-clear checks. Must be pure. On non-OK the prior snapshot is fully
    // preserved, so a rejected run ext cannot destroy the prior transcript.
    //
    // A family SHOULD put its run-ext value validation here so a caller typo
    // is caught pre-clear; the preservation guarantee only covers what this
    // hook actually checks (a family that defers checks to run() trades them
    // out of the guarantee). NULL is skipped; the generic header-size + kind
    // checks remain in force.
    transcribe_status (*run_validate)(const struct transcribe_session * ctx, const transcribe_run_params * params);
};

// Look up an architecture by name. Returns nullptr if no registered
// family matches.
const Arch * find_arch(const char * name);

}  // namespace transcribe
