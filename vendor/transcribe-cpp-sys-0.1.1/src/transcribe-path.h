// transcribe-path.h - UTF-8 path handling for user-facing file access.
//
// Contract: every path crossing the public C ABI is UTF-8 (see
// include/transcribe.h). On POSIX the narrow file APIs consume UTF-8
// bytes directly. On Windows they interpret narrow strings in the
// process ANSI code page, so a non-ASCII path fails with ENOENT even
// though the file exists (Handy issue #1585). Every stat/open a
// user-supplied path can reach must therefore go through these
// helpers, which convert UTF-8 to the native wide encoding on Windows.
//
// Deliberately not converted: dev-only I/O (the transcribe-debug.cpp
// dump writers, the TRANSCRIBE_ENABLE_VALIDATION_HOOKS sidecar
// readers) — those paths never cross the public ABI.

#pragma once

#include <filesystem>
#include <string>
#include <system_error>

namespace transcribe {

// Build a std::filesystem::path from UTF-8 bytes. On Windows this
// converts to the native wide encoding; elsewhere the narrow encoding
// IS UTF-8 and this is a passthrough. (fs::u8path is the C++17
// spelling; it is deprecated-but-present under C++20.)
inline std::filesystem::path path_from_utf8(const char * utf8) {
#if defined(_WIN32)
    return std::filesystem::u8path(utf8);
#else
    return std::filesystem::path(utf8);
#endif
}

inline std::filesystem::path path_from_utf8(const std::string & utf8) {
    return path_from_utf8(utf8.c_str());
}

// Existence pre-check whose ONLY job is to distinguish "the file is
// not at this path" from every other reason a subsequent open might
// fail.
//
// Deliberately no regular-file check: if the path names a directory
// (or a fifo, or a socket), the file IS at that path — the caller's
// open will surface a more accurate error than FILE_NOT_FOUND.
//
// Deliberately only the not-found outcome (ENOENT / ENOTDIR and their
// Windows equivalents, which fs::status maps to file_type::not_found)
// reports absence. Other failures (EACCES, ELOOP, EIO, ...) don't
// prove the file is missing — only that we can't reach it — so they
// return true and the caller's open reports the real error.
inline bool path_is_present(const char * utf8_path) {
    std::error_code ec;
    return std::filesystem::status(path_from_utf8(utf8_path), ec).type() != std::filesystem::file_type::not_found;
}

}  // namespace transcribe
