/*
 * include/transcribe/moonshine_streaming.h - Moonshine-Streaming public
 * extension surface.
 *
 * Includes transcribe.h; safe to include in C or C++ TUs. Holds the
 * Moonshine-Streaming stream extension struct, kind constant, and
 * init function.
 *
 * Probe via transcribe_model_accepts_ext_kind before pointing
 * transcribe_stream_params::family at this struct.
 *
 * FourCC kinds are reserved in docs/extension-kinds.md.
 */

#ifndef TRANSCRIBE_MOONSHINE_STREAMING_H
#define TRANSCRIBE_MOONSHINE_STREAMING_H

#include "transcribe.h"

#ifdef __cplusplus
extern "C" {
#endif

/* 'MSST' little-endian = 0x5453534D */
#define TRANSCRIBE_EXT_KIND_MOONSHINE_STREAMING_STREAM 0x5453534Du

/*
 * Moonshine-Streaming decode-throttle knob.
 *
 *   min_decode_interval_ms
 *
 *     Minimum audio-time interval between expensive autoregressive
 *     partial decodes while a stream is active. This is a Moonshine-
 *     Streaming scheduling knob, not a generic streaming publication
 *     cadence. It can reduce partial-update compute at the cost of
 *     less frequent tentative transcripts.
 *
 *     -1 (default): use the family default cadence.
 *     < -1:         caller bug; transcribe_stream_begin returns
 *                   TRANSCRIBE_ERR_INVALID_ARG.
 *      0:           decode on every stable encoder-frame advance.
 *     > 0:          request at least this many milliseconds between
 *                   partial decodes. Values below one encoder frame
 *                   are rounded up to one frame; other positive values
 *                   are rounded up to the next encoder-frame boundary.
 *
 *     stream_finalize always performs the final decode if needed,
 *     independent of this throttle.
 */
struct transcribe_moonshine_streaming_stream_ext {
    struct transcribe_ext ext;
    int32_t               min_decode_interval_ms;
};

/* Fills ext.size/kind and min_decode_interval_ms = -1 (family default). */
TRANSCRIBE_API void transcribe_moonshine_streaming_stream_ext_init(
    struct transcribe_moonshine_streaming_stream_ext * ext);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* TRANSCRIBE_MOONSHINE_STREAMING_H */
