/*
 * include/transcribe/sortformer.h - Sortformer-family public extension.
 *
 * Includes transcribe.h; safe to include in C or C++ TUs. Sortformer
 * (diar_streaming_sortformer_4spk-v2.1) serves the DIARIZE role
 * (include/transcribe/diarize.h). Its one extension picks the streaming
 * operating point for transcribe_diarize_run; probe with
 * transcribe_model_accepts_ext_kind(model, TRANSCRIBE_EXT_SLOT_DIARIZE_RUN,
 * TRANSCRIBE_EXT_KIND_SORTFORMER_DIARIZE).
 *
 * FourCC kinds are reserved in docs/extension-kinds.md.
 */

#ifndef TRANSCRIBE_SORTFORMER_H
#define TRANSCRIBE_SORTFORMER_H

#include "transcribe.h"

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Streaming operating point (latency / accuracy trade-off).
 *
 * The model processes audio in fixed chunks and carries speaker identity
 * across chunks in a bounded cache; the operating point sets the chunk
 * geometry. Each named preset is a jointly-tuned bundle published by the
 * upstream model (chunk length, lookahead, FIFO and speaker-cache
 * geometry) - the menu is discrete, not a continuous latency dial, and
 * only these bundles are accuracy-validated (AMI DER, see
 * docs/porting/families/sortformer.md).
 *
 *   DEFAULT             The GGUF-shipped checkpoint configuration.
 *   VERY_HIGH_LATENCY   ~30.4 s algorithmic lookahead (chunk 340 + rc 40
 *                       frames @ 80 ms). Highest accuracy; the published
 *                       operating point for offline file processing.
 *   HIGH_LATENCY        ~10.0 s lookahead (chunk 124 + rc 1).
 *   LOW_LATENCY         ~1.04 s lookahead (chunk 6 + rc 7). The
 *                       real-time operating point. Note: much higher
 *                       compute per audio second than the larger chunks
 *                       (many small windows); see the family doc for
 *                       measured throughput.
 *
 * Values outside the enum range are rejected by transcribe_diarize_run
 * with TRANSCRIBE_ERR_INVALID_ARG before the previous result is cleared.
 */
typedef enum {
    TRANSCRIBE_SORTFORMER_PRESET_DEFAULT           = 0,
    TRANSCRIBE_SORTFORMER_PRESET_VERY_HIGH_LATENCY = 1,
    TRANSCRIBE_SORTFORMER_PRESET_HIGH_LATENCY      = 2,
    TRANSCRIBE_SORTFORMER_PRESET_LOW_LATENCY       = 3,
} transcribe_sortformer_preset;

/* 'SFDR' little-endian = 0x52444653 (DIARIZE_RUN slot) */
#define TRANSCRIBE_EXT_KIND_SORTFORMER_DIARIZE 0x52444653u

/* transcribe_diarize_params::family: the operating point for
 * transcribe_diarize_run. */
struct transcribe_sortformer_diarize_ext {
    struct transcribe_ext        ext;
    transcribe_sortformer_preset preset;
};

/* Fills ext.size/kind and preset = DEFAULT. */
TRANSCRIBE_API void transcribe_sortformer_diarize_ext_init(struct transcribe_sortformer_diarize_ext * ext);

#ifdef __cplusplus
}
#endif

#endif /* TRANSCRIBE_SORTFORMER_H */
