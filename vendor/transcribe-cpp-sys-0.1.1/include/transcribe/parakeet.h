/*
 * include/transcribe/parakeet.h - Parakeet-family public extension surface.
 *
 * Includes transcribe.h; safe to include in C or C++ TUs. Holds the
 * streaming extension structs (cache-aware and chunked-attention
 * variants) and their kind constants and init functions.
 *
 * Acceptance is per-loaded-model-variant: nemotron-speech-streaming-en-0.6b
 * (cache-aware) accepts TRANSCRIBE_EXT_KIND_PARAKEET_STREAM and rejects
 * TRANSCRIBE_EXT_KIND_PARAKEET_BUFFERED_STREAM; parakeet-unified-en-0.6b
 * (chunked_limited_with_rc) does the opposite. Probe via
 * transcribe_model_accepts_ext_kind before pointing
 * transcribe_stream_params::family at one of these structs.
 *
 * FourCC kinds are reserved in docs/extension-kinds.md.
 */

#ifndef TRANSCRIBE_PARAKEET_H
#define TRANSCRIBE_PARAKEET_H

#include "transcribe.h"

#ifdef __cplusplus
extern "C" {
#endif

/* 'PKST' little-endian = 0x54534B50 */
#define TRANSCRIBE_EXT_KIND_PARAKEET_STREAM          0x54534B50u
/* 'PKBS' little-endian = 0x53424B50 */
#define TRANSCRIBE_EXT_KIND_PARAKEET_BUFFERED_STREAM 0x53424B50u

/*
 * Cache-aware streaming knob (nemotron-speech-streaming-en-0.6b).
 *
 *   att_context_right
 *
 *     Right-context (lookahead) selector in encoder frames. The cache-
 *     aware streaming variants are trained on a menu of (left, right)
 *     pairs simultaneously - the user picks one at inference time to
 *     trade latency for accuracy. nemotron's published menu is
 *     right ∈ {13, 6, 1, 0}, corresponding to lookahead of
 *     {1040, 480, 80, 0} ms at the 80ms encoder frame rate.
 *
 *     -1 (default): use the model's default setting (first entry of
 *                   att_context_size_choices = max-accuracy /
 *                   max-latency).
 *     < -1:         caller bug; transcribe_stream_begin returns
 *                   TRANSCRIBE_ERR_INVALID_ARG.
 *     >= 0:         select the corresponding (left, att_context_right)
 *                   entry from the model's training menu. 0 is
 *                   legitimate when 0-frame lookahead is in the menu.
 *                   transcribe_stream_begin returns
 *                   TRANSCRIBE_ERR_INVALID_ARG if the requested right
 *                   is not in the menu.
 *
 *     The published menu and the lookahead (in milliseconds) each
 *     entry corresponds to are documented in the model's family doc
 *     (docs/models/nemotron-speech-streaming-en-0.6b.md); -1 selects
 *     the model's default (max-accuracy / max-latency) entry.
 */
struct transcribe_parakeet_stream_ext {
    struct transcribe_ext ext;
    int32_t               att_context_right;
};

/* Fills ext.size/kind and att_context_right = -1 (model default). */
TRANSCRIBE_API void transcribe_parakeet_stream_ext_init(struct transcribe_parakeet_stream_ext * ext);

/*
 * Chunked-attention (buffered) streaming knob (parakeet-unified-en-0.6b).
 *
 * parakeet-unified-en-0.6b is trained with chunked_limited_with_rc
 * attention over a menu of (left, chunk, right) context tuples
 * expressed in 80ms encoder frames. The user picks the active tuple at
 * stream_begin time; the encoder re-runs over each new
 * [left | chunk | right] PCM window. Each field is in MILLISECONDS;
 * the runtime converts to encoder frames at the model's frame rate.
 *
 * Per-field sentinels (each field independently):
 *
 *   -1     "use the model default for this field." Unified-en-0.6b's
 *          best-accuracy default is L=5600 ms / C=1040 ms / R=1040 ms,
 *          which the published WER numbers correspond to.
 *   < -1   caller bug; transcribe_stream_begin returns
 *          TRANSCRIBE_ERR_INVALID_ARG.
 *   0      a legitimate requested value when 0 frames is in the model's
 *          menu for that field. Not all fields admit 0.
 *   > 0    must be an exact positive multiple of the encoder frame size
 *          (80 ms for every shipped FastConformer streaming variant).
 *          A value that does not divide the frame returns
 *          TRANSCRIBE_ERR_INVALID_ARG; the runtime never silently floors.
 *
 * After per-field resolution the (L, C, R) frame tuple is validated
 * against the model's training menu
 * (stt.parakeet.encoder.att_chunk_{left,chunk,right}_choices); tuples
 * outside the menu return TRANSCRIBE_ERR_INVALID_ARG.
 */
struct transcribe_parakeet_buffered_stream_ext {
    struct transcribe_ext ext;
    int32_t               left_ms;
    int32_t               chunk_ms;
    int32_t               right_ms;
};

/* Fills ext.size/kind and left/chunk/right_ms = -1 (model default). */
TRANSCRIBE_API void transcribe_parakeet_buffered_stream_ext_init(struct transcribe_parakeet_buffered_stream_ext * ext);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* TRANSCRIBE_PARAKEET_H */
