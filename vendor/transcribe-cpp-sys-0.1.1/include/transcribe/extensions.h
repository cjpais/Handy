/*
 * include/transcribe/extensions.h - convenience umbrella for family
 * extension surfaces.
 *
 * Include <transcribe.h> when you only need the stable generic ABI.
 * Include <transcribe/extensions.h> when you want all family-specific
 * extension structs, kind constants, init functions, and telemetry
 * helpers shipped by this install. Binding generators should usually parse this
 * header. It is intentionally allowed to grow as new family headers are
 * added; transcribe.h is the stable generic surface.
 */

#ifndef TRANSCRIBE_EXTENSIONS_H
#define TRANSCRIBE_EXTENSIONS_H

#include "transcribe.h"
#include "transcribe/moonshine_streaming.h"
#include "transcribe/parakeet.h"
#include "transcribe/voxtral_realtime.h"
#include "transcribe/whisper.h"

#endif /* TRANSCRIBE_EXTENSIONS_H */
