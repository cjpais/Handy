/*
 * include/transcribe/voxtral_realtime.h - Voxtral Realtime public extension
 * surface.
 *
 * Includes transcribe.h; safe to include in C or C++ TUs. Holds the Voxtral
 * Realtime streaming extension struct, kind constant, and init function.
 *
 * Probe via transcribe_model_accepts_ext_kind before pointing
 * transcribe_stream_params::family at this struct.
 *
 * FourCC kinds are reserved in docs/extension-kinds.md.
 */

#ifndef TRANSCRIBE_VOXTRAL_REALTIME_H
#define TRANSCRIBE_VOXTRAL_REALTIME_H

#include "transcribe.h"

#ifdef __cplusplus
extern "C" {
#endif

/* 'VRST' little-endian = 0x54535256 */
#define TRANSCRIBE_EXT_KIND_VOXTRAL_REALTIME_STREAM 0x54535256u

/*
 * Voxtral Realtime streaming knobs.
 *
 *   num_delay_tokens
 *
 *     Transcription delay in 12.5 Hz audio tokens (80 ms each). The model
 *     emits the token for audio position t after observing audio through
 *     t + num_delay_tokens, trading latency for accuracy. The publisher
 *     default (and the value this port validates against) is 6 (= 480 ms).
 *
 *     The accepted values are the publisher's validated set (model card +
 *     mistral-common): a delay that is a multiple of 80 ms in [80, 1200]
 *     (tokens 1..15) OR the standalone 2400 ms (token 30). Other values
 *     (0, 16..29, > 30) return TRANSCRIBE_ERR_INVALID_ARG even though the
 *     architecture would run them — they are out of the validated range.
 *
 *     -1 (default): use the model default (default_num_delay_tokens, 6).
 *     1..15:        80..1200 ms.
 *     30:           2400 ms.
 *     anything else: TRANSCRIBE_ERR_INVALID_ARG.
 *
 *   min_decode_interval_ms
 *
 *     Minimum audio-time interval between tentative partial decodes while a
 *     stream is ACTIVE. Voxtral Realtime's partial decode reprocesses the
 *     accumulated buffer, so this knob bounds partial-decode compute at the
 *     cost of less frequent tentative transcripts. stream_finalize always
 *     performs the final decode regardless of this throttle, and that final
 *     decode is byte-identical to the offline transcribe_run path.
 *
 *     -1 (default): use the family default cadence.
 *     < -1:         caller bug; transcribe_stream_begin returns
 *                   TRANSCRIBE_ERR_INVALID_ARG.
 *      0:           decode on every feed.
 *     > 0:          request at least this many milliseconds of new audio
 *                   between tentative decodes.
 */
struct transcribe_voxtral_realtime_stream_ext {
    struct transcribe_ext ext;
    int32_t               num_delay_tokens;
    int32_t               min_decode_interval_ms;
};

/*
 * Fills ext.size/kind and num_delay_tokens = -1 (model default),
 * min_decode_interval_ms = -1 (family default).
 */
TRANSCRIBE_API void transcribe_voxtral_realtime_stream_ext_init(struct transcribe_voxtral_realtime_stream_ext * ext);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* TRANSCRIBE_VOXTRAL_REALTIME_H */
