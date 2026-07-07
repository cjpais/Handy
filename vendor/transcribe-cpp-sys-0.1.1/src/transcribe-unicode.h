// transcribe-unicode.h - internal Unicode helpers for the BPE encoder.
//
// Scope: only what the "gpt2" byte-level BPE tokenizer path needs to
// encode text into pretokens. No NFD normalization, no case folding
// beyond ASCII, no general-purpose regex engine. If a future family
// wants a different tokenizer flavor (e.g. LLaMA 3's longer
// contractions alternation, or a pretokenizer that inspects \p{M})
// this module is where the missing pieces land.
//
// This header is INTERNAL. Nothing here leaks through the public C
// ABI; everything lives in the transcribe::unicode namespace and is
// consumed by transcribe-tokenizer.cpp.
//
// Attribution: the codepoint-flag data tables in the sibling
// transcribe-unicode-data.cpp are copied verbatim from
// llama.cpp/src/unicode-data.cpp (MIT). The algorithmic pieces
// (pretokenizer, byte-level map, utf-8 codec) are ported and adapted
// from llama.cpp/src/unicode.cpp with the same license.

#pragma once

#include <cstddef>
#include <cstdint>
#include <string>
#include <vector>

namespace transcribe::unicode {

// Codepoint category flags. Bit layout matches llama.cpp's
// unicode_cpt_flags so the shared data table works as-is; the helpers
// below only inspect the bits we actually need.
struct CptFlags {
    enum : uint16_t {
        UNDEFINED   = 0x0001,
        NUMBER      = 0x0002,  // \p{N}
        LETTER      = 0x0004,  // \p{L}
        SEPARATOR   = 0x0008,  // \p{Z}
        ACCENT_MARK = 0x0010,  // \p{M}
        PUNCTUATION = 0x0020,  // \p{P}
        SYMBOL      = 0x0040,  // \p{S}
        CONTROL     = 0x0080,  // \p{C}
        WHITESPACE  = 0x0100,  // \s (regex whitespace, superset of \p{Z})
    };

    uint16_t bits = 0;

    bool is_letter() const { return (bits & LETTER) != 0; }

    bool is_number() const { return (bits & NUMBER) != 0; }

    bool is_whitespace() const { return (bits & WHITESPACE) != 0; }

    // True if any category bit in MASK_CATEGORIES (the low byte) is
    // set. Mirrors unicode_cpt_flags::as_uint() & MASK_CATEGORIES != 0
    // from llama.cpp. Used by the pretokenizer to distinguish "known
    // character in some category" from "undefined / outside the
    // ranges we care about" (OOR codepoints get bits=0 at lookup).
    bool has_category() const { return (bits & 0x00FFu) != 0; }
};

// UTF-8 codec. Mirrors the narrow subset of llama.cpp's unicode.cpp
// helpers that the pretokenizer needs. Invalid UTF-8 throws
// std::invalid_argument; callers only pass text that already made it
// through the GGUF tokenizer / Python reference, so the throw guards a
// programming bug rather than a user-facing error path.
std::string           cpt_to_utf8(uint32_t cpt);
uint32_t              cpt_from_utf8(const std::string & utf8, size_t & offset);
std::vector<uint32_t> cpts_from_utf8(const std::string & utf8);

// Length in bytes of the UTF-8 sequence whose leading byte is `c`.
// Returns 1 for continuation bytes so callers can inch forward on
// corrupt input without over-running.
size_t len_utf8(char c);

// Codepoint category flags. Falls back to CptFlags{0} (has_category()
// == false) for codepoints outside the Unicode Character Database
// ranges we ship.
CptFlags flags_from_cpt(uint32_t cpt);

// ASCII tolower. The Qwen2 pretokenizer calls tolower only on the
// character immediately after an apostrophe (to match the
// (?i:'s|'t|'re|'ve|'m|'ll|'d) alternation). Those targets are all
// ASCII letters, so ASCII-only folding is sufficient and avoids
// pulling in a second ~1400-line case-map data table.
uint32_t tolower_ascii(uint32_t cpt);

// Byte-level BPE mapping (Python GPT-2 "bytes_to_unicode"). Each of
// the 256 possible input bytes maps to one visible codepoint in
// [0x21..0xFF minus a handful of gaps, plus 0x0100..0x0142] so the
// resulting string has no whitespace or control characters. BPE
// merges run on the byte-encoded string; at detokenize time the
// inverse map recovers the original bytes. The map is deterministic
// and matches transformers.models.gpt2.tokenization_gpt2.
// bytes_to_unicode().
//
// byte_to_unicode(b) returns the UTF-8 encoding of the mapped
// codepoint (1 or 2 bytes). unicode_to_byte(utf8) is the inverse; it
// returns -1 if `utf8` is not a one-codepoint encoding in the mapped
// range, so callers can distinguish "plain byte-encoded glyph" from
// "synthetic UTF-8 we produced out of thin air."
std::string byte_to_unicode(uint8_t byte);
int         unicode_to_byte(const std::string & utf8);

// Split `text` into Qwen2 pretokens, then byte-encode each one.
// Each output string is a pretoken ready for the BPE merge loop.
//
// The Qwen2 pattern is:
//   (?:'[sS]|'[tT]|'[rR][eE]|'[vV][eE]|'[mM]|'[lL][lL]|'[dD])
//   | [^\r\n\p{L}\p{N}]? \p{L}+
//   | \p{N}
//   | " "? [^\s\p{L}\p{N}]+ [\r\n]*
//   | \s* [\r\n]+
//   | \s+ (?!\S)
//   | \s+
//
// Implementation walks codepoints with the CptFlags table; it does
// not use std::regex. Adapted from
// unicode_regex_split_custom_qwen2() in llama.cpp's unicode.cpp.
std::vector<std::string> pretokenize_qwen2(const std::string & text);

// Split `text` into GPT-2 pretokens, then byte-encode each one. Same
// output shape as pretokenize_qwen2; the split rule is the original
// GPT-2 regex used by HuggingFace's tokenizers crate for ByteLevel
// pretokenizers (Whisper, GPT-2, RoBERTa, CLIP, ...):
//
//   's | 't | 're | 've | 'm | 'll | 'd
//   |  ?\p{L}+
//   |  ?\p{N}+
//   |  ?[^\s\p{L}\p{N}]+
//   | \s+(?!\S)
//   | \s+
//
// Differences from the Qwen2 variant:
//   1. Contractions are CASE-SENSITIVE (lowercase only), not (?i:).
//   2. Letter / number / symbol runs are greedy (`+`) and take an
//      optional leading space; Qwen2 limits numbers to one codepoint
//      and lets symbol runs swallow trailing `[\r\n]*`.
//   3. No `\s*[\r\n]+` special case.
//
// These differences are load-bearing for any digit-containing text:
// HF tokenizes " 123" as one pretoken → one BPE piece; Qwen2 would
// split into three single-digit pretokens and block the merge.
std::vector<std::string> pretokenize_gpt2(const std::string & text);

// Same regex split as pretokenize_gpt2, but each pretoken is the raw
// UTF-8 substring of the input — no byte-to-unicode remap. Used by
// the tiktoken-style encoder for whisper.cpp `.bin` vocabularies,
// which store tokens as raw byte sequences rather than the GPT-2
// remapped form.
std::vector<std::string> pretokenize_gpt2_raw_bytes(const std::string & text);

// IBM Granite-4 / gpt-oss pretokenizer. Almost identical to the Qwen2
// regex but symbol runs do NOT swallow trailing `[\r\n]*`; instead
// each newline starts a fresh pretoken. The granite tokenizer.json
// regex is:
//
//   (?i:'s|'t|'re|'ve|'m|'ll|'d)
//   | [^\r\n\p{L}\p{N}]? \p{L}+
//   | \p{N}{1,3}
//   |  ?[^\s\p{L}\p{N}]+ [\r\n]*
//   | \s* [\r\n]+
//   | \s+ (?!\S)
//   | \s+
//
// In the HF tokenizers crate that regex tokenizes "?\n" as TWO
// pretokens ("?" then "\n") rather than the one chunk the Python
// `regex` module produces — alt 4's `[\r\n]*` does not actually
// consume the trailing newline in HF's regex engine. This split
// matters because the granite BPE has a merge for "?+\n" (token
// 5380); applying that merge produces a token id the LM was not
// trained to see right after "format". Without this granite-specific
// pretokenizer we'd emit 5380 where the reference produces (30, 198).
std::vector<std::string> pretokenize_granite(const std::string & text);

}  // namespace transcribe::unicode
