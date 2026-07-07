// transcribe-log.h - internal printf-style logging facility.
//
// Routes a formatted message through the process-global log callback
// installed via transcribe_log_set(), falling back to stderr when none is
// installed. This is the supported way for library internals (including
// per-family run() drivers) to surface diagnostics; family code must NOT
// call std::fprintf(stderr, ...) directly, or a consumer's log sink would
// never see the message. The implementation lives in transcribe.cpp
// alongside the callback globals it reads.

#pragma once

#include "transcribe.h"  // transcribe_log_level

namespace transcribe {

// Emit a printf-style formatted message at `level`. The format/args are
// rendered into a bounded stack buffer (oversize messages are truncated,
// never heap-allocated). Routed through the installed log callback; if
// none is installed the rendered line is written to stderr with a
// trailing newline, matching transcribe_print_timings' fallback.
void log_msg(transcribe_log_level level, const char * fmt, ...)
#if defined(__GNUC__) || defined(__clang__)
    __attribute__((format(printf, 2, 3)))
#endif
    ;

}  // namespace transcribe
