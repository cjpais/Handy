// transcribe-tokenizer.cpp - internal tokenizer implementation.
//
// See transcribe-tokenizer.h for the contract. This file knows nothing
// about model families: it just reads tokenizer.ggml.* keys and exposes
// id <-> piece, decode(), and encode() (gpt2 only).
//
// Strict-now contract on KV reads: a tokenizer.ggml.* key that is
// present but has the wrong GGUF type is treated as a converter bug
// and surfaced as TRANSCRIBE_ERR_GGUF, not silently ignored. The
// helpers in transcribe-meta.h provide the KvResult tri-state that
// makes this distinction observable.

#include "transcribe-tokenizer.h"

#include "gguf.h"
#include "transcribe-log.h"
#include "transcribe-meta.h"
#include "transcribe-unicode.h"

#include <algorithm>
#include <cstdio>
#include <cstring>
#include <queue>
#include <string>
#include <vector>

namespace transcribe {

namespace {

// SentencePiece word-boundary marker, U+2581 "LOWER ONE EIGHTH BLOCK".
// UTF-8: 0xE2 0x96 0x81. Used by NeMo Parakeet (and most other recent
// SentencePiece-tokenized models) as the visible "leading space"
// prefix of any token that starts a new word.
constexpr const char k_sp_space[]   = "\xE2\x96\x81";
constexpr int        k_sp_space_len = 3;

// Separator used in merge_rank_ keys. U+001F (unit separator) is a
// C0 control character that never appears in a byte-level BPE token
// because the GPT-2 byte map moves all C0/C1 into the
// 0x0100-0x0142 range before BPE sees them.
constexpr char k_merge_sep = '\x1F';

// Read a typed-element scalar array KV. ExpectedType is the gguf_type
// the array elements should have; T is the C++ type the loader copies
// them into. Returns KvResult so the caller can distinguish "absent"
// from "present but wrong array element type". Scoped to this file
// because the only consumers are scores and token_type below.
template <typename T>
KvResult read_typed_array_kv(const gguf_context * ctx,
                             const char *         key,
                             gguf_type            expected_element_type,
                             std::vector<T> &     out) {
    const int64_t key_id = gguf_find_key(ctx, key);
    if (key_id < 0) {
        return KvResult::Absent;
    }
    if (gguf_get_kv_type(ctx, key_id) != GGUF_TYPE_ARRAY) {
        return KvResult::BadType;
    }
    if (gguf_get_arr_type(ctx, key_id) != expected_element_type) {
        return KvResult::BadType;
    }

    const size_t   n = gguf_get_arr_n(ctx, key_id);
    std::vector<T> tmp(n);
    if (n > 0) {
        const void * data = gguf_get_arr_data(ctx, key_id);
        if (data == nullptr) {
            return KvResult::BadType;
        }
        std::memcpy(tmp.data(), data, n * sizeof(T));
    }
    out = std::move(tmp);
    return KvResult::Ok;
}

}  // namespace

// ---------------------------------------------------------------------------
// Vocabulary accessors.
// ---------------------------------------------------------------------------

const std::string & Tokenizer::token(int id) const {
    static const std::string k_empty;
    if (id < 0 || static_cast<size_t>(id) >= tokens_.size()) {
        return k_empty;
    }
    return tokens_[static_cast<size_t>(id)];
}

bool Tokenizer::is_control(int id) const {
    // llama.cpp / scripts/lib/gguf_common.py convention.
    constexpr int32_t k_token_type_control = 3;
    if (id < 0 || static_cast<size_t>(id) >= token_type_.size()) {
        return false;
    }
    return token_type_[static_cast<size_t>(id)] == k_token_type_control;
}

int Tokenizer::find(const std::string & piece) const {
    const auto it = piece_to_id_.find(piece);
    if (it != piece_to_id_.end()) {
        return it->second;
    }
    // Synthesized special-piece map: only populated by adapters that
    // need find() to surface literals absent from tokens_ (whisper.cpp
    // .bin). Empty for GGUF whisper, where every special is already
    // in piece_to_id_.
    const auto sit = special_pieces_.find(piece);
    if (sit != special_pieces_.end()) {
        return sit->second;
    }
    return -1;
}

void Tokenizer::add_special_piece(const std::string & literal, int32_t id) {
    special_pieces_[literal] = id;
}

bool Tokenizer::has_encoder() const {
    // GGUF "gpt2" with merges → HF byte-unicode + merge-rank encoder.
    // Decode-only raw-bytes (whisper.cpp .bin) → tiktoken-style
    // encoder using piece_to_id_ as the rank table.
    if (model_ == "gpt2" && !merge_rank_.empty()) {
        return true;
    }
    if (decode_mode_ == DecodeMode::RawBytes && !tokens_.empty()) {
        return true;
    }
    return false;
}

// ---------------------------------------------------------------------------
// decode() - id sequence -> text.
// ---------------------------------------------------------------------------

namespace {

// SentencePiece-convention decode: replace U+2581 with ASCII space
// byte-for-byte. Common path for "unigram" / "bpe".
//
// Also handles SentencePiece byte-fallback tokens of the form "<0xHH>"
// (six characters: '<', '0', 'x', two hex digits, '>'). When the source
// vocabulary doesn't contain a glyph, SentencePiece emits one byte token
// per UTF-8 byte and the detokenizer reassembles them. English-only
// models never hit this path (ASCII is always in-vocab); non-English
// variants of the same architecture do, on every character that needs
// it (Vietnamese diacritics, all CJK, Cyrillic outside the base set,
// etc.). Without this reassembly the model output contains literal
// "<0xE1><0xBB><0x95>" instead of the intended Unicode character.
std::string decode_sentencepiece(const std::vector<std::string> & tokens, const int * ids, int n) {
    auto hex_nibble = [](char c) -> int {
        if (c >= '0' && c <= '9') {
            return c - '0';
        }
        if (c >= 'A' && c <= 'F') {
            return 10 + (c - 'A');
        }
        if (c >= 'a' && c <= 'f') {
            return 10 + (c - 'a');
        }
        return -1;
    };
    auto try_byte_fallback = [&](const std::string & p) -> int {
        if (p.size() != 6) {
            return -1;
        }
        if (p[0] != '<' || p[1] != '0' || p[2] != 'x' || p[5] != '>') {
            return -1;
        }
        const int hi = hex_nibble(p[3]);
        const int lo = hex_nibble(p[4]);
        if (hi < 0 || lo < 0) {
            return -1;
        }
        return (hi << 4) | lo;
    };

    std::string out;
    out.reserve(static_cast<size_t>(n) * 4);
    for (int i = 0; i < n; ++i) {
        const int id = ids[i];
        if (id < 0 || static_cast<size_t>(id) >= tokens.size()) {
            continue;
        }
        const std::string & p    = tokens[static_cast<size_t>(id)];
        const int           byte = try_byte_fallback(p);
        if (byte >= 0) {
            out.push_back(static_cast<char>(byte));
            continue;
        }
        size_t j = 0;
        while (j < p.size()) {
            if (j + k_sp_space_len <= p.size() && std::memcmp(p.data() + j, k_sp_space, k_sp_space_len) == 0) {
                out.push_back(' ');
                j += k_sp_space_len;
            } else {
                out.push_back(p[j]);
                ++j;
            }
        }
    }
    return out;
}

// Raw-byte decode: tokens already contain raw UTF-8 byte sequences
// (legacy whisper.cpp .bin tiktoken-style vocab). Concatenate verbatim.
std::string decode_raw_bytes(const std::vector<std::string> & tokens, const int * ids, int n) {
    std::string out;
    out.reserve(static_cast<size_t>(n) * 2);
    for (int i = 0; i < n; ++i) {
        const int id = ids[i];
        if (id < 0 || static_cast<size_t>(id) >= tokens.size()) {
            continue;
        }
        out.append(tokens[static_cast<size_t>(id)]);
    }
    return out;
}

// GPT-2 byte-level decode: invert the bytes_to_unicode mapping per
// codepoint to recover raw UTF-8 bytes.
std::string decode_gpt2_bytes(const std::vector<std::string> & tokens, const int * ids, int n) {
    std::string out;
    out.reserve(static_cast<size_t>(n) * 2);
    std::string one_cpt;
    one_cpt.reserve(4);
    for (int i = 0; i < n; ++i) {
        const int id = ids[i];
        if (id < 0 || static_cast<size_t>(id) >= tokens.size()) {
            continue;
        }
        const std::string & p = tokens[static_cast<size_t>(id)];
        size_t              j = 0;
        while (j < p.size()) {
            const size_t w = std::min(p.size() - j, unicode::len_utf8(p[j]));
            one_cpt.assign(p.data() + j, w);
            const int byte = unicode::unicode_to_byte(one_cpt);
            if (byte >= 0) {
                out.push_back(static_cast<char>(byte));
            } else {
                // Not a byte-level glyph: pass the raw UTF-8 through.
                // This handles special tokens that the decode driver
                // failed to filter out (e.g. "<|im_end|>") -- better
                // to surface the literal bytes than drop silently.
                out.append(p, j, w);
            }
            j += w;
        }
    }
    return out;
}

}  // namespace

std::string Tokenizer::decode(const int * ids, int n) const {
    if (ids == nullptr || n <= 0) {
        return {};
    }
    switch (decode_mode_) {
        case DecodeMode::Gpt2ByteUnicode:
            return decode_gpt2_bytes(tokens_, ids, n);
        case DecodeMode::RawBytes:
            return decode_raw_bytes(tokens_, ids, n);
        case DecodeMode::SentencePiece:
            break;
    }
    return decode_sentencepiece(tokens_, ids, n);
}

// ---------------------------------------------------------------------------
// encode() - text -> id sequence.
// ---------------------------------------------------------------------------

namespace {

// One symbol in the linked-list workspace used by the BPE merge loop.
// `text` is a view into the pretoken's byte-encoded string (owned by
// the caller). `n == 0` means "this slot has been merged away" and
// should be skipped at output time. prev/next index into the same
// vector.
struct Symbol {
    const char * text = nullptr;
    size_t       n    = 0;
    int          prev = -1;
    int          next = -1;
};

// One candidate merge in the priority queue. The invariant we rely on
// is that a queue entry may become stale after a prior merge: when we
// pop it, we re-check by concatenating the two current symbol texts
// and comparing to `text`. Mismatched entries are silently dropped.
struct Bigram {
    int         left  = -1;
    int         right = -1;
    std::string text;
    int         rank = 0;
    size_t      size = 0;
};

struct BigramGreater {
    bool operator()(const Bigram & a, const Bigram & b) const {
        // Lower rank wins; on ties, lower `left` index wins (same
        // tiebreaker as llama.cpp's comparator).
        if (a.rank != b.rank) {
            return a.rank > b.rank;
        }
        return a.left > b.left;
    }
};

}  // namespace

// Tiktoken-style encoder for raw-bytes vocabularies (whisper.cpp .bin):
//   - pretokens are pure UTF-8 byte sequences (no GPT-2 byte-unicode remap),
//   - symbols are seeded one per byte (not one per codepoint),
//   - merge ranks come from piece_to_id_ directly: the rank of "ab" is
//     piece_to_id_["ab"] (lower id = lower rank = higher merge priority).
//
// This is the algorithm OpenAI tiktoken uses: there is no separate
// merges list, the vocab order IS the merge order. Splitting/merging
// emits whatever the BPE-encoded sequence is for the model's training-
// time tokenization.
//
// Defined as a free function over the Tokenizer state so encode() can
// dispatch into it cleanly.
namespace {

transcribe_status encode_tiktoken_raw_bytes(const std::string &                              text,
                                            const std::string &                              pre,
                                            const std::unordered_map<std::string, int32_t> & piece_to_id,
                                            std::vector<int32_t> &                           out_ids) {
    out_ids.clear();
    if (text.empty()) {
        return TRANSCRIBE_OK;
    }

    std::vector<std::string> words;
    if (pre == "gpt2" || pre.empty()) {
        words = transcribe::unicode::pretokenize_gpt2_raw_bytes(text);
    } else {
        // Whisper / tiktoken uses the GPT-2 regex; falling back to
        // raw bytes without a regex would produce a different
        // tokenization. Reject loudly.
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "tokenizer.encode: pretokenizer \"%s\" not "
                "supported with raw-bytes vocab",
                pre.c_str());
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    auto rank_of_concat = [&](const std::string & left, const std::string & right) -> int {
        std::string key;
        key.reserve(left.size() + right.size());
        key.append(left);
        key.append(right);
        const auto it = piece_to_id.find(key);
        return (it == piece_to_id.end()) ? -1 : it->second;
    };

    std::vector<Symbol> symbols;
    symbols.reserve(64);
    std::priority_queue<Bigram, std::vector<Bigram>, BigramGreater> queue;

    auto push_bigram = [&](int l, int r) {
        if (l < 0 || r < 0) {
            return;
        }
        const std::string left(symbols[l].text, symbols[l].n);
        const std::string right(symbols[r].text, symbols[r].n);
        const int         rank = rank_of_concat(left, right);
        if (rank < 0) {
            return;
        }
        Bigram bg;
        bg.left  = l;
        bg.right = r;
        bg.text  = left + right;
        bg.rank  = rank;
        bg.size  = left.size() + right.size();
        queue.push(std::move(bg));
    };

    out_ids.reserve(words.size() * 2);
    for (const auto & word : words) {
        while (!queue.empty()) {
            queue.pop();
        }
        symbols.clear();

        // Seed: one symbol per BYTE (not codepoint). Tiktoken-style
        // BPE works at byte granularity, with single-byte tokens
        // forming the bottom of the rank ladder.
        for (size_t off = 0; off < word.size(); ++off) {
            Symbol s;
            s.text = word.data() + off;
            s.n    = 1;
            s.prev = static_cast<int>(off) - 1;
            s.next = (off + 1 == word.size()) ? -1 : static_cast<int>(off) + 1;
            symbols.push_back(s);
        }

        for (int i = 1; i < static_cast<int>(symbols.size()); ++i) {
            push_bigram(i - 1, i);
        }

        while (!queue.empty()) {
            Bigram bg = queue.top();
            queue.pop();
            Symbol & left  = symbols[bg.left];
            Symbol & right = symbols[bg.right];
            if (left.n == 0 || right.n == 0) {
                continue;
            }
            if (left.n + right.n != bg.size) {
                continue;
            }
            if (std::memcmp(left.text, bg.text.data(), left.n) != 0) {
                continue;
            }
            if (std::memcmp(right.text, bg.text.data() + left.n, right.n) != 0) {
                continue;
            }

            left.n += right.n;
            right.n   = 0;
            left.next = right.next;
            if (right.next >= 0) {
                symbols[right.next].prev = bg.left;
            }
            push_bigram(left.prev, bg.left);
            push_bigram(bg.left, left.next);
        }

        for (int i = 0; i != -1; i = symbols[i].next) {
            const Symbol & s = symbols[i];
            if (s.n == 0) {
                continue;
            }
            const std::string piece(s.text, s.n);
            const auto        it = piece_to_id.find(piece);
            if (it != piece_to_id.end()) {
                out_ids.push_back(it->second);
                continue;
            }
            // Single-byte fallback: every byte should be in the
            // vocab as a base-rank token. If not, the .bin is
            // structurally broken.
            for (size_t j = 0; j < piece.size(); ++j) {
                const std::string sub(piece, j, 1);
                const auto        it2 = piece_to_id.find(sub);
                if (it2 == piece_to_id.end()) {
                    log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "tokenizer.encode: byte 0x%02X not in "
                            "vocab",
                            static_cast<unsigned>(static_cast<unsigned char>(piece[j])));
                    return TRANSCRIBE_ERR_GGUF;
                }
                out_ids.push_back(it2->second);
            }
        }
    }
    return TRANSCRIBE_OK;
}

}  // namespace

transcribe_status Tokenizer::encode(const std::string & text, std::vector<int32_t> & out_ids) const {
    out_ids.clear();

    // Raw-bytes mode: tiktoken-style encoder, no merge table needed.
    // Set by load_decode_only_raw_bytes for whisper.cpp .bin files.
    if (decode_mode_ == DecodeMode::RawBytes) {
        if (tokens_.empty()) {
            return TRANSCRIBE_ERR_GGUF;
        }
        return encode_tiktoken_raw_bytes(text, pre_, piece_to_id_, out_ids);
    }

    if (model_ != "gpt2") {
        return TRANSCRIBE_ERR_NOT_IMPLEMENTED;
    }
    if (merge_rank_.empty()) {
        // GGUF "gpt2" tokenizer without merges array. encode()
        // can't run a greedy merge loop without ranks; fall back to
        // the byte-level decomposition would produce correct but
        // under-merged output (every character its own token), which
        // is never what a caller wants. Surface the configuration
        // gap instead.
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "tokenizer.encode: \"gpt2\" tokenizer loaded without "
                "tokenizer.ggml.merges; encode unavailable");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (text.empty()) {
        return TRANSCRIBE_OK;
    }

    // Pretokenize into byte-encoded "words". Dispatch on the
    // pretokenizer flavor (set at load time from tokenizer.ggml.pre,
    // or overridden by a per-family loader via set_pretokenizer).
    // Unknown flavors fall back to qwen2 with a warning — keeps old
    // GGUFs that lack the key working.
    std::vector<std::string> words;
    if (pre_ == "gpt2") {
        words = unicode::pretokenize_gpt2(text);
    } else if (pre_ == "granite") {
        words = unicode::pretokenize_granite(text);
    } else {
        if (pre_ != "qwen2" && !pre_.empty()) {
            log_msg(TRANSCRIBE_LOG_LEVEL_WARN,
                    "tokenizer.encode: unknown tokenizer.ggml.pre "
                    "\"%s\"; falling back to qwen2",
                    pre_.c_str());
        }
        words = unicode::pretokenize_qwen2(text);
    }

    // Run the BPE merge loop per word and collect ids.
    out_ids.reserve(words.size() * 2);

    std::vector<Symbol> symbols;
    symbols.reserve(64);
    std::priority_queue<Bigram, std::vector<Bigram>, BigramGreater> queue;

    auto rank_of = [&](const std::string & left, const std::string & right) -> int {
        std::string key;
        key.reserve(left.size() + 1 + right.size());
        key.append(left);
        key.push_back(k_merge_sep);
        key.append(right);
        const auto it = merge_rank_.find(key);
        return (it == merge_rank_.end()) ? -1 : it->second;
    };

    auto push_bigram = [&](int l, int r) {
        if (l < 0 || r < 0) {
            return;
        }
        const std::string left(symbols[l].text, symbols[l].n);
        const std::string right(symbols[r].text, symbols[r].n);
        const int         rank = rank_of(left, right);
        if (rank < 0) {
            return;
        }
        Bigram bg;
        bg.left  = l;
        bg.right = r;
        bg.text  = left + right;
        bg.rank  = rank;
        bg.size  = left.size() + right.size();
        queue.push(std::move(bg));
    };

    for (const auto & word : words) {
        // Drain any stale entries from a previous word.
        while (!queue.empty()) {
            queue.pop();
        }
        symbols.clear();

        // Seed: one symbol per UTF-8 codepoint of the byte-encoded
        // word. Codepoint-level chunking matches the Python reference
        // (HF tokenizers iterate codepoints before merging).
        int    index  = 0;
        size_t offset = 0;
        while (offset < word.size()) {
            const size_t clen = std::min(word.size() - offset, unicode::len_utf8(word[offset]));
            Symbol       s;
            s.text = word.data() + offset;
            s.n    = clen;
            s.prev = index - 1;
            s.next = (offset + clen == word.size()) ? -1 : index + 1;
            symbols.push_back(s);
            offset += clen;
            ++index;
        }

        for (int i = 1; i < static_cast<int>(symbols.size()); ++i) {
            push_bigram(i - 1, i);
        }

        while (!queue.empty()) {
            Bigram bg = queue.top();
            queue.pop();

            Symbol & left  = symbols[bg.left];
            Symbol & right = symbols[bg.right];
            if (left.n == 0 || right.n == 0) {
                continue;
            }

            // Stale-entry check: if either side has been merged into
            // something else since this bigram was pushed, the
            // concatenation won't match `bg.text` and we drop it.
            if (left.n + right.n != bg.size) {
                continue;
            }
            if (std::memcmp(left.text, bg.text.data(), left.n) != 0) {
                continue;
            }
            if (std::memcmp(right.text, bg.text.data() + left.n, right.n) != 0) {
                continue;
            }

            // Merge right into left; unlink right.
            left.n += right.n;
            right.n   = 0;
            left.next = right.next;
            if (right.next >= 0) {
                symbols[right.next].prev = bg.left;
            }

            // Enqueue new neighbor bigrams around the enlarged left.
            push_bigram(left.prev, bg.left);
            push_bigram(bg.left, left.next);
        }

        for (int i = 0; i != -1; i = symbols[i].next) {
            const Symbol & s = symbols[i];
            if (s.n == 0) {
                continue;
            }
            const std::string piece(s.text, s.n);
            const auto        it = piece_to_id_.find(piece);
            if (it != piece_to_id_.end()) {
                out_ids.push_back(it->second);
                continue;
            }
            // BPE didn't land on a whole-vocab token — decompose into
            // single-byte tokens. Each byte of the byte-encoded
            // pretoken is itself a one-codepoint piece that the
            // vocab carries; the converter guarantees this.
            for (size_t j = 0; j < piece.size(); /*++j*/) {
                const size_t      clen = std::min(piece.size() - j, unicode::len_utf8(piece[j]));
                const std::string sub(piece, j, clen);
                const auto        it2 = piece_to_id_.find(sub);
                if (it2 == piece_to_id_.end()) {
                    log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "tokenizer.encode: byte-fallback piece "
                            "not in vocab (\"%s\")",
                            sub.c_str());
                    return TRANSCRIBE_ERR_GGUF;
                }
                out_ids.push_back(it2->second);
                j += clen;
            }
        }
    }
    return TRANSCRIBE_OK;
}

// ---------------------------------------------------------------------------
// load() - populate the vocab / merges / special ids from GGUF.
// ---------------------------------------------------------------------------

transcribe_status Tokenizer::load(const gguf_context * gguf) {
    if (gguf == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // Required: model type. Both Absent and BadType are fatal — a
    // missing model string and a model string with the wrong GGUF
    // type are both "we cannot trust this file's tokenizer payload."
    switch (read_string_kv(gguf, "tokenizer.ggml.model", model_)) {
        case KvResult::Absent:
        case KvResult::BadType:
            return TRANSCRIBE_ERR_GGUF;
        case KvResult::Ok:
            break;
    }

    // Accepted: "unigram"/"bpe" (SentencePiece flavors used by NeMo
    // Parakeet and Cohere ASR), "gpt2" (llama.cpp's tag for
    // byte-level BPE, used by Qwen3-ASR and Whisper), and "char"
    // (charwise vocab where every id maps to its piece string verbatim;
    // used by GigaAM's non-e2e variants). Per-family encode/decode
    // paths branch on model_. Recognized-but-unsupported strings
    // surface as NOT_IMPLEMENTED so the caller can tell "the file is
    // fine, the library is just not ready for this tokenizer" from
    // "the file is broken."
    if (model_ != "unigram" && model_ != "bpe" && model_ != "gpt2" && model_ != "char") {
        return TRANSCRIBE_ERR_NOT_IMPLEMENTED;
    }
    if (model_ == "gpt2") {
        decode_mode_ = DecodeMode::Gpt2ByteUnicode;
    } else if (model_ == "char") {
        // Charwise: piece strings are literal characters; concatenation
        // is the entire decode. RawBytes mode does exactly that.
        decode_mode_ = DecodeMode::RawBytes;
    } else {
        decode_mode_ = DecodeMode::SentencePiece;
    }

    // Optional: pretokenizer flavor. Absent means "qwen2" (the default
    // for the "gpt2" model tag on Qwen3-ASR GGUFs). Per-family loaders
    // override after load() when the source file did not emit the key but
    // the family's pretokenizer is fixed (Whisper → "gpt2").
    pre_.clear();
    switch (read_string_kv(gguf, "tokenizer.ggml.pre", pre_)) {
        case KvResult::Absent:
            pre_ = "qwen2";
            break;
        case KvResult::BadType:
            return TRANSCRIBE_ERR_GGUF;
        case KvResult::Ok:
            break;
    }

    // Required: tokens array. Same Absent/BadType semantics.
    switch (read_string_array_kv(gguf, "tokenizer.ggml.tokens", tokens_)) {
        case KvResult::Absent:
        case KvResult::BadType:
            return TRANSCRIBE_ERR_GGUF;
        case KvResult::Ok:
            break;
    }
    if (tokens_.empty()) {
        return TRANSCRIBE_ERR_GGUF;
    }

    // Build the piece -> id hash. First-occurrence wins (matches the
    // forward tokens_ ordering that token(id) exposes). special_pieces_
    // is cleared too: GGUFs carry every special as a real vocab entry,
    // so the synthesized-special fallback should be empty here. Hygiene
    // matters if a Tokenizer instance is ever reused across loads.
    piece_to_id_.clear();
    special_pieces_.clear();
    piece_to_id_.reserve(tokens_.size() * 2);
    for (size_t i = 0; i < tokens_.size(); ++i) {
        piece_to_id_.emplace(tokens_[i], static_cast<int32_t>(i));
    }

    // Optional: scores. Absent is fine (we clear and move on); BadType
    // is a converter bug and propagates as ERR_GGUF. The length-must-
    // match contract on Ok is a separate, narrower check.
    switch (read_typed_array_kv<float>(gguf, "tokenizer.ggml.scores", GGUF_TYPE_FLOAT32, scores_)) {
        case KvResult::Absent:
            scores_.clear();
            break;
        case KvResult::BadType:
            return TRANSCRIBE_ERR_GGUF;
        case KvResult::Ok:
            if (scores_.size() != tokens_.size()) {
                return TRANSCRIBE_ERR_GGUF;
            }
            break;
    }

    // Optional: token_type.
    switch (read_typed_array_kv<int32_t>(gguf, "tokenizer.ggml.token_type", GGUF_TYPE_INT32, token_type_)) {
        case KvResult::Absent:
            token_type_.clear();
            break;
        case KvResult::BadType:
            return TRANSCRIBE_ERR_GGUF;
        case KvResult::Ok:
            if (token_type_.size() != tokens_.size()) {
                return TRANSCRIBE_ERR_GGUF;
            }
            break;
    }

    // Optional: merges. Only "gpt2" uses them; SentencePiece tokenizers
    // ("unigram"/"bpe") encode via a score-based lattice (not
    // implemented) and their decode path needs no merges. Missing merges
    // on a "gpt2" file means encode() fails at call time -- the right
    // surface, since the decode path still works.
    merge_rank_.clear();
    std::vector<std::string> merges;
    switch (read_string_array_kv(gguf, "tokenizer.ggml.merges", merges)) {
        case KvResult::Absent:
            break;
        case KvResult::BadType:
            return TRANSCRIBE_ERR_GGUF;
        case KvResult::Ok:
            merge_rank_.reserve(merges.size() * 2);
            for (size_t i = 0; i < merges.size(); ++i) {
                const std::string & line = merges[i];
                // Format is "left<space>right". The split scans from
                // position 1 so a merge whose left side is a single
                // space (" ") still parses correctly — matches
                // llama.cpp's bpe_ranks loader.
                const size_t        sp   = line.find(' ', 1);
                if (sp == std::string::npos) {
                    log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "tokenizer.load: merge %zu has no space "
                            "separator: \"%s\"",
                            i, line.c_str());
                    return TRANSCRIBE_ERR_GGUF;
                }
                std::string key;
                key.reserve(line.size());
                key.append(line, 0, sp);
                key.push_back(k_merge_sep);
                key.append(line, sp + 1, std::string::npos);
                merge_rank_.emplace(std::move(key), static_cast<int32_t>(i));
            }
            break;
    }

    // Optional special token ids.
    auto read_special = [&](const char * key, int & field) -> transcribe_status {
        switch (read_token_id_kv(gguf, key, field)) {
            case KvResult::Absent:
                return TRANSCRIBE_OK;
            case KvResult::Ok:
                return TRANSCRIBE_OK;
            case KvResult::BadType:
                return TRANSCRIBE_ERR_GGUF;
        }
        return TRANSCRIBE_ERR_GGUF;
    };

    if (auto st = read_special("tokenizer.ggml.unknown_token_id", unk_id_); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_special("tokenizer.ggml.bos_token_id", bos_id_); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_special("tokenizer.ggml.eos_token_id", eos_id_); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_special("tokenizer.ggml.blank_token_id", blank_id_); st != TRANSCRIBE_OK) {
        return st;
    }

    return TRANSCRIBE_OK;
}

// ---------------------------------------------------------------------------
// load_decode_only_* - in-memory vocab loaders for non-GGUF sources.
// ---------------------------------------------------------------------------

namespace {

// Shared vocab + specials installation for the decode-only loaders.
// Leaves model_ / pre_ unset; the per-mode caller picks decode_mode_
// after this returns. Encode availability follows from decode_mode_:
// the RawBytes loader (whisper.cpp .bin) gets a tiktoken-style
// encoder via piece_to_id_ ranks; the Gpt2ByteUnicode decode-only
// loader (no merges populated) reports has_encoder() == false.
transcribe_status install_decode_only_vocab(std::vector<std::string> &                 tokens_dst,
                                            std::unordered_map<std::string, int32_t> & piece_to_id_dst,
                                            std::vector<std::string>                   tokens_src) {
    if (tokens_src.empty()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    tokens_dst = std::move(tokens_src);
    piece_to_id_dst.clear();
    piece_to_id_dst.reserve(tokens_dst.size() * 2);
    for (size_t i = 0; i < tokens_dst.size(); ++i) {
        piece_to_id_dst.emplace(tokens_dst[i], static_cast<int32_t>(i));
    }
    return TRANSCRIBE_OK;
}

}  // namespace

transcribe_status Tokenizer::load_decode_only_gpt2(std::vector<std::string>   tokens,
                                                   const DecodeOnlySpecials & specials) {
    if (auto st = install_decode_only_vocab(tokens_, piece_to_id_, std::move(tokens)); st != TRANSCRIBE_OK) {
        return st;
    }

    decode_mode_ = DecodeMode::Gpt2ByteUnicode;
    model_.clear();
    pre_.clear();
    scores_.clear();
    token_type_.clear();
    merge_rank_.clear();
    special_pieces_.clear();

    unk_id_   = specials.unk_id;
    bos_id_   = specials.bos_id;
    eos_id_   = specials.eos_id;
    blank_id_ = specials.blank_id;
    return TRANSCRIBE_OK;
}

transcribe_status Tokenizer::load_decode_only_raw_bytes(std::vector<std::string>   tokens,
                                                        const DecodeOnlySpecials & specials) {
    if (auto st = install_decode_only_vocab(tokens_, piece_to_id_, std::move(tokens)); st != TRANSCRIBE_OK) {
        return st;
    }

    decode_mode_ = DecodeMode::RawBytes;
    model_.clear();
    pre_.clear();
    scores_.clear();
    token_type_.clear();
    merge_rank_.clear();
    special_pieces_.clear();

    unk_id_   = specials.unk_id;
    bos_id_   = specials.bos_id;
    eos_id_   = specials.eos_id;
    blank_id_ = specials.blank_id;
    return TRANSCRIBE_OK;
}

}  // namespace transcribe
