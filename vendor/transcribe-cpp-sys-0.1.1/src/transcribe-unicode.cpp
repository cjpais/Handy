// transcribe-unicode.cpp - Unicode helpers for the byte-level BPE path.
//
// Implementation of transcribe-unicode.h. Pieces ported from
// llama.cpp/src/unicode.cpp (MIT); see that file for the full
// multi-pretokenizer dispatch. We carry only what the Qwen2 pretok
// (and thus Qwen3-ASR's byte-level BPE encode) needs.

#include "transcribe-unicode.h"

#include <algorithm>
#include <cassert>
#include <cstdint>
#include <initializer_list>
#include <stdexcept>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <utility>
#include <vector>

namespace transcribe::unicode {

// ---------------------------------------------------------------------------
// Data tables (defined in transcribe-unicode-data.cpp).
// ---------------------------------------------------------------------------

namespace data {
extern const std::initializer_list<std::pair<uint32_t, uint16_t>> ranges_flags;
extern const std::unordered_set<uint32_t>                         set_whitespace;
}  // namespace data

// ---------------------------------------------------------------------------
// UTF-8 codec.
// ---------------------------------------------------------------------------

size_t len_utf8(char c) {
    // Fast leading-byte classifier. The top 4 bits pick one of 16
    // buckets; bytes in the continuation form (0x80-0xBF) map to 1
    // so a caller walking blindly still makes forward progress on
    // corrupt input.
    static const size_t lookup[16] = {
        1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 3, 4,
    };
    const uint8_t hi = static_cast<uint8_t>(c) >> 4;
    return lookup[hi];
}

std::string cpt_to_utf8(uint32_t cpt) {
    std::string out;
    if (cpt <= 0x7F) {
        out.push_back(static_cast<char>(cpt));
        return out;
    }
    if (cpt <= 0x7FF) {
        out.push_back(static_cast<char>(0xC0 | (cpt >> 6)));
        out.push_back(static_cast<char>(0x80 | (cpt & 0x3F)));
        return out;
    }
    if (cpt <= 0xFFFF) {
        out.push_back(static_cast<char>(0xE0 | (cpt >> 12)));
        out.push_back(static_cast<char>(0x80 | ((cpt >> 6) & 0x3F)));
        out.push_back(static_cast<char>(0x80 | (cpt & 0x3F)));
        return out;
    }
    if (cpt <= 0x10FFFF) {
        out.push_back(static_cast<char>(0xF0 | (cpt >> 18)));
        out.push_back(static_cast<char>(0x80 | ((cpt >> 12) & 0x3F)));
        out.push_back(static_cast<char>(0x80 | ((cpt >> 6) & 0x3F)));
        out.push_back(static_cast<char>(0x80 | (cpt & 0x3F)));
        return out;
    }
    throw std::invalid_argument("transcribe::unicode: codepoint out of range");
}

uint32_t cpt_from_utf8(const std::string & utf8, size_t & offset) {
    assert(offset < utf8.size());
    const auto b0 = static_cast<uint8_t>(utf8[offset]);
    if ((b0 & 0x80u) == 0) {
        offset += 1;
        return b0;
    }
    if ((b0 & 0xE0u) == 0xC0u) {
        if (offset + 1 >= utf8.size() || (static_cast<uint8_t>(utf8[offset + 1]) & 0xC0u) != 0x80u) {
            throw std::invalid_argument("transcribe::unicode: malformed UTF-8 (2-byte)");
        }
        uint32_t c = (static_cast<uint32_t>(b0 & 0x1Fu) << 6) | (static_cast<uint32_t>(utf8[offset + 1]) & 0x3Fu);
        offset += 2;
        return c;
    }
    if ((b0 & 0xF0u) == 0xE0u) {
        if (offset + 2 >= utf8.size() || (static_cast<uint8_t>(utf8[offset + 1]) & 0xC0u) != 0x80u ||
            (static_cast<uint8_t>(utf8[offset + 2]) & 0xC0u) != 0x80u) {
            throw std::invalid_argument("transcribe::unicode: malformed UTF-8 (3-byte)");
        }
        uint32_t c = (static_cast<uint32_t>(b0 & 0x0Fu) << 12) |
                     ((static_cast<uint32_t>(utf8[offset + 1]) & 0x3Fu) << 6) |
                     (static_cast<uint32_t>(utf8[offset + 2]) & 0x3Fu);
        offset += 3;
        return c;
    }
    if ((b0 & 0xF8u) == 0xF0u) {
        if (offset + 3 >= utf8.size() || (static_cast<uint8_t>(utf8[offset + 1]) & 0xC0u) != 0x80u ||
            (static_cast<uint8_t>(utf8[offset + 2]) & 0xC0u) != 0x80u ||
            (static_cast<uint8_t>(utf8[offset + 3]) & 0xC0u) != 0x80u) {
            throw std::invalid_argument("transcribe::unicode: malformed UTF-8 (4-byte)");
        }
        uint32_t c = (static_cast<uint32_t>(b0 & 0x07u) << 18) |
                     ((static_cast<uint32_t>(utf8[offset + 1]) & 0x3Fu) << 12) |
                     ((static_cast<uint32_t>(utf8[offset + 2]) & 0x3Fu) << 6) |
                     (static_cast<uint32_t>(utf8[offset + 3]) & 0x3Fu);
        offset += 4;
        return c;
    }
    throw std::invalid_argument("transcribe::unicode: malformed UTF-8 lead byte");
}

std::vector<uint32_t> cpts_from_utf8(const std::string & utf8) {
    std::vector<uint32_t> out;
    out.reserve(utf8.size());
    size_t i = 0;
    while (i < utf8.size()) {
        out.push_back(cpt_from_utf8(utf8, i));
    }
    return out;
}

// ---------------------------------------------------------------------------
// Category flags.
// ---------------------------------------------------------------------------
//
// The flags table is a list of half-open [start, next_start) ranges
// with the flags that apply to every codepoint in the range. We
// binary-search `cpt` against the starts and return the flags of the
// enclosing range. A second pass ORs WHITESPACE in for the codepoints
// that the Unicode whitespace set picks out (\s is a superset of
// \p{Z} in Python's re, so we can't derive this purely from
// category). The table is static, so the whitespace union only
// materializes once per process.

namespace {

// Flat [MAX_COMMON_CPT] lookup for the first portion of the codepoint
// space. Binary search is O(log N) with N≈2274 — fine for the handful
// of calls the pretokenizer makes per utterance — but for ASCII/Latin
// we take the fast path via an array. Covers the bulk of realistic
// inputs at zero per-lookup comparisons.
constexpr uint32_t k_fast_path_limit = 0x10000u;  // BMP only

struct FlagsTable {
    std::vector<std::pair<uint32_t, uint16_t>> ranges;
    std::vector<uint16_t>                      fast;  // size = k_fast_path_limit

    FlagsTable() {
        ranges.reserve(data::ranges_flags.size());
        for (const auto & rf : data::ranges_flags) {
            ranges.emplace_back(rf.first, rf.second);
        }
        // Overlay whitespace bit (Unicode `\s` regex class, a superset
        // of \p{Z}). Iterate the separator-whitespace exceptions.
        // In-place OR: we do this per codepoint on range entries that
        // cover them.
        // Build the fast-path array.
        fast.assign(k_fast_path_limit, 0);
        for (size_t i = 1; i < ranges.size(); ++i) {
            const auto     begin = ranges[i - 1].first;
            const auto     end   = ranges[i].first;
            const auto     flag  = ranges[i - 1].second;
            const uint32_t b     = begin;
            const uint32_t e     = std::min<uint32_t>(end, k_fast_path_limit);
            for (uint32_t c = b; c < e; ++c) {
                fast[c] = flag;
            }
            if (end >= k_fast_path_limit) {
                break;
            }
        }
        for (uint32_t ws : data::set_whitespace) {
            if (ws < k_fast_path_limit) {
                fast[ws] = static_cast<uint16_t>(fast[ws] | CptFlags::WHITESPACE);
            }
        }
    }

    uint16_t lookup(uint32_t cpt) const {
        if (cpt < k_fast_path_limit) {
            return fast[cpt];
        }
        // Binary search: find the largest range.first <= cpt.
        size_t lo = 0;
        size_t hi = ranges.size();
        while (lo + 1 < hi) {
            const size_t mid = (lo + hi) / 2;
            if (ranges[mid].first <= cpt) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        uint16_t f = ranges[lo].second;
        if (data::set_whitespace.count(cpt)) {
            f = static_cast<uint16_t>(f | CptFlags::WHITESPACE);
        }
        return f;
    }
};

const FlagsTable & flags_table() {
    static const FlagsTable t;
    return t;
}

}  // namespace

CptFlags flags_from_cpt(uint32_t cpt) {
    CptFlags f;
    f.bits = flags_table().lookup(cpt);
    return f;
}

uint32_t tolower_ascii(uint32_t cpt) {
    if (cpt >= 'A' && cpt <= 'Z') {
        return cpt + ('a' - 'A');
    }
    return cpt;
}

// ---------------------------------------------------------------------------
// Byte-level BPE ↔ map.
// ---------------------------------------------------------------------------
//
// Python reference (transformers.models.gpt2.tokenization_gpt2):
//
//   bs = list(range(ord("!"), ord("~")+1)) +
//        list(range(ord("¡"), ord("¬")+1)) +
//        list(range(ord("®"), ord("ÿ")+1))
//   cs = bs.copy(); n = 0
//   for b in range(256):
//       if b not in bs:
//           bs.append(b); cs.append(256+n); n += 1
//   cs = [chr(c) for c in cs]
//   return dict(zip(bs, cs))
//
// We compute the same mapping at static-init time as a pair of
// 256-entry arrays. Forward: uint8 byte -> UTF-8 string (1 or 2
// bytes). Inverse: UTF-8 codepoint (up to 0x142) -> byte, with -1 for
// codepoints that aren't in the mapping.

namespace {

struct ByteMap {
    std::string                           fwd[256];  // byte -> utf-8 of mapped codepoint
    uint32_t                              cpt[256];  // byte -> mapped codepoint
    // Inverse: the mapped codepoints are dense in two ranges
    // (0x21..0xFF minus 0x7F/0xAD, plus 0x0100..0x0142). We pack the
    // inverse into a small open-addressing map keyed by codepoint.
    std::unordered_map<uint32_t, uint8_t> inv;

    ByteMap() {
        // Build the "visible" byte list (bs) and the codepoint list (cs).
        std::vector<int> bs;
        bs.reserve(256);
        for (int b = '!'; b <= '~'; ++b) {
            bs.push_back(b);
        }
        for (int b = 0xA1; b <= 0xAC; ++b) {
            bs.push_back(b);
        }
        for (int b = 0xAE; b <= 0xFF; ++b) {
            bs.push_back(b);
        }

        std::vector<int> cs = bs;
        int              n  = 0;
        for (int b = 0; b < 256; ++b) {
            if (std::find(bs.begin(), bs.end(), b) == bs.end()) {
                bs.push_back(b);
                cs.push_back(256 + n);
                ++n;
            }
        }
        assert(bs.size() == 256 && cs.size() == 256);
        for (size_t i = 0; i < bs.size(); ++i) {
            const uint8_t  byte = static_cast<uint8_t>(bs[i]);
            const uint32_t c    = static_cast<uint32_t>(cs[i]);
            fwd[byte]           = cpt_to_utf8(c);
            cpt[byte]           = c;
            inv.emplace(c, byte);
        }
    }
};

const ByteMap & byte_map() {
    static const ByteMap m;
    return m;
}

}  // namespace

std::string byte_to_unicode(uint8_t byte) {
    return byte_map().fwd[byte];
}

int unicode_to_byte(const std::string & utf8) {
    if (utf8.empty()) {
        return -1;
    }
    size_t   off = 0;
    uint32_t c;
    try {
        c = cpt_from_utf8(utf8, off);
    } catch (const std::invalid_argument &) {
        return -1;
    }
    if (off != utf8.size()) {
        return -1;  // must be a single-codepoint piece
    }
    const auto & m  = byte_map().inv;
    const auto   it = m.find(c);
    if (it == m.end()) {
        return -1;
    }
    return static_cast<int>(it->second);
}

// ---------------------------------------------------------------------------
// Qwen2 pretokenizer.
// ---------------------------------------------------------------------------
//
// Adapted from unicode_regex_split_custom_qwen2 in llama.cpp. Returns
// the pretoken spans as byte-encoded strings so callers can feed them
// straight into the BPE merge loop. Each output piece is what GPT-2
// BPE would call a "word" -- a run of byte-encoded glyphs that BPE
// is allowed to merge within; merges never cross pretoken boundaries.

namespace {

constexpr uint32_t OOR = 0xFFFFFFFFu;

// Collect pretoken end offsets (in codepoints) for the Qwen2 regex.
// `begin` and `end` are the cpt-space bounds we are pretokenizing;
// they stay as one contiguous range here because we don't have the
// special-token partition step -- our inputs never contain <|...|>
// special tokens (those live in the chat template ids we resolve
// out of band).
std::vector<size_t> qwen2_split_offsets(const std::vector<uint32_t> & cpts, size_t begin, size_t end) {
    std::vector<size_t> out;

    auto get_cpt = [&](size_t p) -> uint32_t {
        return (begin <= p && p < end) ? cpts[p] : OOR;
    };
    auto get_flags = [&](size_t p) -> CptFlags {
        return (begin <= p && p < end) ? flags_from_cpt(cpts[p]) : CptFlags{};
    };

    size_t prev      = begin;
    auto   push_upto = [&](size_t p) {
        assert(prev <= p && p <= end);
        if (p > prev) {
            out.push_back(p);
            prev = p;
        }
    };

    size_t pos = begin;
    while (pos < end) {
        const uint32_t cpt   = get_cpt(pos);
        const CptFlags flags = get_flags(pos);

        // (?:'[sS]|'[tT]|'[rR][eE]|'[vV][eE]|'[mM]|'[lL][lL]|'[dD])
        if (cpt == '\'' && pos + 1 < end) {
            const uint32_t c1 = tolower_ascii(get_cpt(pos + 1));
            if (c1 == 's' || c1 == 't' || c1 == 'm' || c1 == 'd') {
                pos += 2;
                push_upto(pos);
                continue;
            }
            if (pos + 2 < end) {
                const uint32_t c2 = tolower_ascii(get_cpt(pos + 2));
                if ((c1 == 'r' && c2 == 'e') || (c1 == 'v' && c2 == 'e') || (c1 == 'l' && c2 == 'l')) {
                    pos += 3;
                    push_upto(pos);
                    continue;
                }
            }
        }

        // [^\r\n\p{L}\p{N}]? \p{L}+
        if (!(cpt == '\r' || cpt == '\n' || flags.is_number())) {
            const bool self_is_letter = flags.is_letter();
            const bool next_is_letter = get_flags(pos + 1).is_letter();
            if (self_is_letter || next_is_letter) {
                pos++;  // consume the single leading non-letter/non-number or first letter
                while (get_flags(pos).is_letter()) {
                    pos++;
                }
                push_upto(pos);
                continue;
            }
        }

        // \p{N}   (Qwen2 is single-digit-per-pretoken, unlike GPT-2's \p{N}+)
        if (flags.is_number()) {
            pos++;
            push_upto(pos);
            continue;
        }

        // " "? [^\s\p{L}\p{N}]+ [\r\n]*
        {
            CptFlags f2 = (cpt == ' ') ? get_flags(pos + 1) : flags;
            if (!(f2.is_whitespace() || f2.is_letter() || f2.is_number()) && flags.has_category()) {
                pos += (cpt == ' ' ? 1 : 0);
                while (true) {
                    const CptFlags fx = get_flags(pos);
                    if (fx.is_whitespace() || fx.is_letter() || fx.is_number() || !fx.has_category()) {
                        break;
                    }
                    pos++;
                }
                uint32_t cn = get_cpt(pos);
                while (cn == '\r' || cn == '\n') {
                    pos++;
                    cn = get_cpt(pos);
                }
                push_upto(pos);
                continue;
            }
        }

        // Whitespace handling: \s*[\r\n]+ | \s+(?!\S) | \s+
        size_t ws_count      = 0;
        size_t last_after_nl = 0;  // "end-of-\s*[\r\n]+" if any
        while (get_flags(pos + ws_count).is_whitespace()) {
            const uint32_t cw = get_cpt(pos + ws_count);
            if (cw == '\r' || cw == '\n') {
                last_after_nl = pos + ws_count + 1;
            }
            ws_count++;
        }

        // \s*[\r\n]+  (longest whitespace run that ends in a newline)
        if (last_after_nl > 0) {
            pos = last_after_nl;
            push_upto(pos);
            continue;
        }
        // \s+(?!\S)  (whitespace not followed by non-whitespace; leaves 1 space for next token)
        if (ws_count > 1 && get_cpt(pos + ws_count) != OOR) {
            pos += ws_count - 1;
            push_upto(pos);
            continue;
        }
        // \s+
        if (ws_count > 0) {
            pos += ws_count;
            push_upto(pos);
            continue;
        }

        // No match: drop one codepoint on the floor as its own pretoken.
        // llama.cpp's pretokenizer does the same; this protects against
        // adversarial inputs the regex engine would also not match.
        pos++;
        push_upto(pos);
    }

    return out;
}

}  // namespace

std::vector<std::string> pretokenize_qwen2(const std::string & text) {
    std::vector<std::string> out;
    if (text.empty()) {
        return out;
    }
    const auto cpts = cpts_from_utf8(text);
    const auto ends = qwen2_split_offsets(cpts, 0, cpts.size());

    out.reserve(ends.size());
    size_t prev = 0;
    for (size_t e : ends) {
        std::string encoded;
        encoded.reserve((e - prev) * 2);
        for (size_t i = prev; i < e; ++i) {
            // Emit each codepoint as its UTF-8 bytes, then run each raw
            // byte through the byte-level map. This is equivalent to
            // `utf-8 bytes -> byte_to_unicode` per byte, matching the
            // Python transformers pipeline.
            const std::string u = cpt_to_utf8(cpts[i]);
            for (char c : u) {
                encoded += byte_to_unicode(static_cast<uint8_t>(c));
            }
        }
        out.emplace_back(std::move(encoded));
        prev = e;
    }
    return out;
}

// ---------------------------------------------------------------------------
// Granite (gpt-oss style) pretokenizer.
// ---------------------------------------------------------------------------

namespace {

// Same as qwen2_split_offsets except alt 4 stops at the punctuation
// run — it does NOT swallow trailing `[\r\n]*`. Matches the HF
// tokenizers crate's interpretation of the granite tokenizer.json
// regex (`tokenizers` Rust regex engine does not consume the
// `[\r\n]*` here even though the Python `regex` module does).
std::vector<size_t> granite_split_offsets(const std::vector<uint32_t> & cpts, size_t begin, size_t end) {
    std::vector<size_t> out;

    auto get_cpt = [&](size_t p) -> uint32_t {
        return (begin <= p && p < end) ? cpts[p] : OOR;
    };
    auto get_flags = [&](size_t p) -> CptFlags {
        return (begin <= p && p < end) ? flags_from_cpt(cpts[p]) : CptFlags{};
    };

    size_t prev      = begin;
    auto   push_upto = [&](size_t p) {
        assert(prev <= p && p <= end);
        if (p > prev) {
            out.push_back(p);
            prev = p;
        }
    };

    size_t pos = begin;
    while (pos < end) {
        const uint32_t cpt   = get_cpt(pos);
        const CptFlags flags = get_flags(pos);

        // Contractions (case-insensitive).
        if (cpt == '\'' && pos + 1 < end) {
            const uint32_t c1 = tolower_ascii(get_cpt(pos + 1));
            if (c1 == 's' || c1 == 't' || c1 == 'm' || c1 == 'd') {
                pos += 2;
                push_upto(pos);
                continue;
            }
            if (pos + 2 < end) {
                const uint32_t c2 = tolower_ascii(get_cpt(pos + 2));
                if ((c1 == 'r' && c2 == 'e') || (c1 == 'v' && c2 == 'e') || (c1 == 'l' && c2 == 'l')) {
                    pos += 3;
                    push_upto(pos);
                    continue;
                }
            }
        }

        // [^\r\n\p{L}\p{N}]? \p{L}+
        if (!(cpt == '\r' || cpt == '\n' || flags.is_number())) {
            const bool self_is_letter = flags.is_letter();
            const bool next_is_letter = get_flags(pos + 1).is_letter();
            if (self_is_letter || next_is_letter) {
                pos++;
                while (get_flags(pos).is_letter()) {
                    pos++;
                }
                push_upto(pos);
                continue;
            }
        }

        // \p{N}{1,3}  (1-3 digits, like qwen2 but multi-digit is fine
        // up to 3; HF granite uses {1,3}).
        if (flags.is_number()) {
            size_t n = 0;
            while (n < 3 && get_flags(pos + n).is_number()) {
                ++n;
            }
            pos += n;
            push_upto(pos);
            continue;
        }

        // " "? [^\s\p{L}\p{N}]+    (NO trailing [\r\n]* — that's the
        // granite-specific divergence from qwen2.)
        {
            CptFlags f2 = (cpt == ' ') ? get_flags(pos + 1) : flags;
            if (!(f2.is_whitespace() || f2.is_letter() || f2.is_number()) && flags.has_category()) {
                pos += (cpt == ' ' ? 1 : 0);
                while (true) {
                    const CptFlags fx = get_flags(pos);
                    if (fx.is_whitespace() || fx.is_letter() || fx.is_number() || !fx.has_category()) {
                        break;
                    }
                    pos++;
                }
                push_upto(pos);
                continue;
            }
        }

        // Whitespace handling: \s*[\r\n]+ | \s+(?!\S) | \s+
        size_t ws_count      = 0;
        size_t last_after_nl = 0;
        while (get_flags(pos + ws_count).is_whitespace()) {
            const uint32_t cw = get_cpt(pos + ws_count);
            if (cw == '\r' || cw == '\n') {
                last_after_nl = pos + ws_count + 1;
            }
            ws_count++;
        }
        if (last_after_nl > 0) {
            pos = last_after_nl;
            push_upto(pos);
            continue;
        }
        if (ws_count > 1 && get_cpt(pos + ws_count) != OOR) {
            pos += ws_count - 1;
            push_upto(pos);
            continue;
        }
        if (ws_count > 0) {
            pos += ws_count;
            push_upto(pos);
            continue;
        }

        // Fallback: emit one codepoint as its own pretoken.
        pos++;
        push_upto(pos);
    }

    return out;
}

}  // namespace

std::vector<std::string> pretokenize_granite(const std::string & text) {
    std::vector<std::string> out;
    if (text.empty()) {
        return out;
    }
    const auto cpts = cpts_from_utf8(text);
    const auto ends = granite_split_offsets(cpts, 0, cpts.size());

    out.reserve(ends.size());
    size_t prev = 0;
    for (size_t e : ends) {
        std::string encoded;
        encoded.reserve((e - prev) * 2);
        for (size_t i = prev; i < e; ++i) {
            const std::string u = cpt_to_utf8(cpts[i]);
            for (char c : u) {
                encoded += byte_to_unicode(static_cast<uint8_t>(c));
            }
        }
        out.emplace_back(std::move(encoded));
        prev = e;
    }
    return out;
}

// ---------------------------------------------------------------------------
// GPT-2 pretokenizer.
// ---------------------------------------------------------------------------
//
// Matches the regex HuggingFace's tokenizers crate uses for ByteLevel
// pretokenizers (see tokenizers/src/pre_tokenizers/byte_level.rs and
// the equivalent split in OpenAI's tiktoken gpt2 pattern). The
// alternatives are tried in order; the first that matches at the
// current position wins.

namespace {

std::vector<size_t> gpt2_split_offsets(const std::vector<uint32_t> & cpts, size_t begin, size_t end) {
    std::vector<size_t> out;

    auto get_cpt = [&](size_t p) -> uint32_t {
        return (begin <= p && p < end) ? cpts[p] : OOR;
    };
    auto get_flags = [&](size_t p) -> CptFlags {
        return (begin <= p && p < end) ? flags_from_cpt(cpts[p]) : CptFlags{};
    };

    size_t prev      = begin;
    auto   push_upto = [&](size_t p) {
        assert(prev <= p && p <= end);
        if (p > prev) {
            out.push_back(p);
            prev = p;
        }
    };

    size_t pos = begin;
    while (pos < end) {
        const uint32_t cpt = get_cpt(pos);

        // 1. Lowercase contractions (case-SENSITIVE, unlike Qwen2).
        //    's | 't | 're | 've | 'm | 'll | 'd
        if (cpt == '\'' && pos + 1 < end) {
            const uint32_t c1 = get_cpt(pos + 1);
            if (c1 == 's' || c1 == 't' || c1 == 'm' || c1 == 'd') {
                pos += 2;
                push_upto(pos);
                continue;
            }
            if (pos + 2 < end) {
                const uint32_t c2 = get_cpt(pos + 2);
                if ((c1 == 'r' && c2 == 'e') || (c1 == 'v' && c2 == 'e') || (c1 == 'l' && c2 == 'l')) {
                    pos += 3;
                    push_upto(pos);
                    continue;
                }
            }
        }

        // 2. Optional leading space + letter run:   ?\p{L}+
        {
            const bool   leading_space = (cpt == ' ');
            const size_t body          = pos + (leading_space ? 1 : 0);
            if (get_flags(body).is_letter()) {
                size_t q = body;
                while (get_flags(q).is_letter()) {
                    ++q;
                }
                pos = q;
                push_upto(pos);
                continue;
            }
        }

        // 3. Optional leading space + digit run:    ?\p{N}+
        //    This is the bit that matters for Whisper parity with HF
        //    — Qwen2 emits one pretoken per digit, GPT-2 merges the run.
        {
            const bool   leading_space = (cpt == ' ');
            const size_t body          = pos + (leading_space ? 1 : 0);
            if (get_flags(body).is_number()) {
                size_t q = body;
                while (get_flags(q).is_number()) {
                    ++q;
                }
                pos = q;
                push_upto(pos);
                continue;
            }
        }

        // 4. Optional leading space + non-\s\L\N run:   ?[^\s\p{L}\p{N}]+
        //    "Symbols and punctuation", roughly. Stops as soon as a
        //    whitespace / letter / digit / OOR codepoint appears.
        {
            const bool     leading_space = (cpt == ' ');
            const size_t   body          = pos + (leading_space ? 1 : 0);
            const CptFlags fb            = get_flags(body);
            const bool     body_ok =
                body < end && !fb.is_whitespace() && !fb.is_letter() && !fb.is_number() && fb.has_category();
            if (body_ok) {
                size_t q = body;
                while (q < end) {
                    const CptFlags fx = get_flags(q);
                    if (fx.is_whitespace() || fx.is_letter() || fx.is_number() || !fx.has_category()) {
                        break;
                    }
                    ++q;
                }
                pos = q;
                push_upto(pos);
                continue;
            }
        }

        // 5. Whitespace handling: \s+(?!\S) | \s+
        //    The first alternative matches a whitespace run that is
        //    NOT followed by a non-whitespace codepoint. Combined with
        //    the greedy `\s+` fallback, the effective behavior:
        //      - A whitespace run followed by a non-whitespace cpt
        //        emits one-pretoken-per-leading-space plus a final
        //        trailing space (the last space in the run gets
        //        attached to the next letter/number/symbol run via
        //        its optional-leading-space handling above).
        //      - A whitespace run that runs to end-of-input (or that
        //        is followed only by more whitespace) is emitted as a
        //        single pretoken covering the whole run.
        //
        //    Per regex-engine semantics, \s+(?!\S) is tried first and
        //    the engine finds the LONGEST prefix of the run that
        //    satisfies the lookahead. If the run is followed by \S,
        //    the lookahead can only be satisfied by stopping one
        //    codepoint before the end of the run — so \s+(?!\S)
        //    consumes one less codepoint than \s+ would, and the
        //    remaining single whitespace matches the fallback \s+
        //    on the next iteration (becoming the optional leading
        //    space of the following letter/number/symbol pretoken).
        if (get_flags(pos).is_whitespace()) {
            size_t q = pos;
            while (q < end && get_flags(q).is_whitespace()) {
                ++q;
            }
            // q is the end of the full whitespace run.
            const bool followed_by_nonspace = (q < end) && !get_flags(q).is_whitespace();
            if (followed_by_nonspace) {
                // Emit one pretoken per whitespace codepoint up to the
                // second-to-last. The final whitespace is left for
                // the next iteration to consume as leading-space-of-
                // following-pretoken. If the run is a single
                // whitespace, the fallback \s+ takes it as a
                // one-cpt pretoken.
                if (q - pos >= 2) {
                    size_t tail_start = q - 1;
                    while (pos < tail_start) {
                        pos += 1;
                        push_upto(pos);
                    }
                    // Now pos == tail_start; the remaining single
                    // whitespace will be picked up on the next
                    // iteration by either the optional-leading-space
                    // branch of the next alternative or the \s+ fallback.
                } else {
                    // Single whitespace run followed by non-whitespace:
                    // fallback \s+ matches 1 cpt. It becomes its own
                    // pretoken.
                    pos += 1;
                    push_upto(pos);
                }
            } else {
                // Whitespace run to EOF or followed by more whitespace
                // (which cannot happen since we walked until the first
                // non-whitespace anyway) → emit as a single pretoken
                // covering the whole run.
                pos = q;
                push_upto(pos);
            }
            continue;
        }

        // No match: drop one codepoint on the floor as its own
        // pretoken. Mirrors the defensive branch in pretokenize_qwen2.
        pos++;
        push_upto(pos);
    }

    return out;
}

}  // namespace

std::vector<std::string> pretokenize_gpt2(const std::string & text) {
    std::vector<std::string> out;
    if (text.empty()) {
        return out;
    }
    const auto cpts = cpts_from_utf8(text);
    const auto ends = gpt2_split_offsets(cpts, 0, cpts.size());

    out.reserve(ends.size());
    size_t prev = 0;
    for (size_t e : ends) {
        std::string encoded;
        encoded.reserve((e - prev) * 2);
        for (size_t i = prev; i < e; ++i) {
            const std::string u = cpt_to_utf8(cpts[i]);
            for (char c : u) {
                encoded += byte_to_unicode(static_cast<uint8_t>(c));
            }
        }
        out.emplace_back(std::move(encoded));
        prev = e;
    }
    return out;
}

std::vector<std::string> pretokenize_gpt2_raw_bytes(const std::string & text) {
    std::vector<std::string> out;
    if (text.empty()) {
        return out;
    }
    const auto cpts = cpts_from_utf8(text);
    const auto ends = gpt2_split_offsets(cpts, 0, cpts.size());

    out.reserve(ends.size());
    size_t prev = 0;
    for (size_t e : ends) {
        std::string raw;
        raw.reserve(e - prev);
        for (size_t i = prev; i < e; ++i) {
            raw += cpt_to_utf8(cpts[i]);
        }
        out.emplace_back(std::move(raw));
        prev = e;
    }
    return out;
}

}  // namespace transcribe::unicode
