// transcribe-bin-loader.cpp - whisper.cpp `.bin` parser implementation.

#include "transcribe-bin-loader.h"

#include "ggml-backend.h"
#include "ggml.h"
#include "transcribe-log.h"
#include "transcribe-path.h"

#include <cstdio>
#include <cstring>
#include <fstream>
#include <ios>

namespace transcribe::bin_loader {

namespace {

constexpr const char * kTag = "whisper-bin";

// Read sizeof(T) bytes into `out`. Returns false on short read or
// stream failure.
template <typename T> bool read_pod(std::ifstream & fin, T & out) {
    fin.read(reinterpret_cast<char *>(&out), sizeof(T));
    return static_cast<bool>(fin);
}

// Map `ftype % 1000` (the GGML_FTYPE_* enum) to a ggml_type. Whisper
// .bin files write the `ftype` global once in the hparams; per-tensor
// `ttype` is independent and uses ggml_type values directly. This
// helper only validates that a given ttype is a ggml_type our loader
// knows how to size. We accept the same set the linear/conv allowlists
// expose plus a few extras whisper.cpp emits (Q5_0/Q5_1).
bool is_known_ggml_type(int32_t ttype) {
    switch (ttype) {
        case GGML_TYPE_F32:
        case GGML_TYPE_F16:
        case GGML_TYPE_BF16:
        case GGML_TYPE_Q4_0:
        case GGML_TYPE_Q4_1:
        case GGML_TYPE_Q5_0:
        case GGML_TYPE_Q5_1:
        case GGML_TYPE_Q8_0:
        case GGML_TYPE_Q4_K:
        case GGML_TYPE_Q5_K:
        case GGML_TYPE_Q6_K:
        case GGML_TYPE_Q8_K:
        case GGML_TYPE_Q2_K:
        case GGML_TYPE_Q3_K:
            return true;
        default:
            return false;
    }
}

// Size in bytes of a tensor with the given (type, ne[]) using ggml's
// block-quant accounting. Mirrors ggml_nbytes for a freshly created
// tensor: bytes = (n_elements / blck_size) * type_size.
uint64_t tensor_nbytes(ggml_type type, const int64_t ne[4]) {
    const int64_t n_elem = ne[0] * (ne[1] > 0 ? ne[1] : 1) * (ne[2] > 0 ? ne[2] : 1) * (ne[3] > 0 ? ne[3] : 1);
    const int64_t blck   = ggml_blck_size(type);
    if (blck <= 0) {
        return 0;
    }
    if (n_elem % blck != 0) {
        return 0;
    }
    return static_cast<uint64_t>(n_elem / blck) * static_cast<uint64_t>(ggml_type_size(type));
}

// Whisper-shape gate. The `ggml` magic byte is shared with non-
// Whisper artifacts (notably Silero VAD), so after reading the 11
// hparam ints we sanity-check the geometry. The gate's job is to
// reject obviously-non-Whisper files (Silero comes back with
// n_audio_layer=131072, n_mels=8454144 — values out by orders of
// magnitude); it is intentionally permissive about layer counts so
// distilled / turbo variants with shrunk decoders (e.g. distil-medium
// with n_text_layer=2, large-v3-turbo with n_text_layer=4) pass.
//
// The discriminating signals are n_mels (whisper uses 80 or 128
// across every shipped variant) and n_vocab (51864 / 51865 / 51866
// for HF-compatible vocabularies). A future fine-tune outside those
// vocab sizes would need a small extension here.
bool hparams_look_whisper(const WhisperBinHParams & hp) {
    auto layer_ok = [](int32_t l) {
        return l > 0 && l <= 64;
    };
    if (!layer_ok(hp.n_audio_layer)) {
        return false;
    }
    if (!layer_ok(hp.n_text_layer)) {
        return false;
    }
    if (hp.n_mels != 80 && hp.n_mels != 128) {
        return false;
    }
    if (hp.n_vocab != 51864 && hp.n_vocab != 51865 && hp.n_vocab != 51866) {
        return false;
    }
    if (hp.n_audio_state <= 0 || hp.n_audio_head <= 0) {
        return false;
    }
    if (hp.n_audio_state % hp.n_audio_head != 0) {
        return false;
    }
    if (hp.n_text_state <= 0 || hp.n_text_head <= 0) {
        return false;
    }
    if (hp.n_text_state % hp.n_text_head != 0) {
        return false;
    }
    // n_audio_ctx and n_text_ctx are 1500 / 448 across all shipped
    // variants; permit anything plausible to keep the gate from
    // rejecting a hypothetical fine-tune with a different n_text_ctx.
    if (hp.n_audio_ctx <= 0 || hp.n_audio_ctx > 4096) {
        return false;
    }
    if (hp.n_text_ctx <= 0 || hp.n_text_ctx > 4096) {
        return false;
    }

    const int32_t qntvr = hp.ftype / 1000;
    const int32_t base  = hp.ftype % 1000;
    (void) qntvr;
    if (base < 0 || base > 30) {
        return false;
    }
    return true;
}

}  // namespace

transcribe_status parse_whisper_bin(const char * path, WhisperBinModel & out) {
    out = WhisperBinModel{};

    if (path == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (!path_is_present(path)) {
        return TRANSCRIBE_ERR_FILE_NOT_FOUND;
    }
    out.path = path;

    std::ifstream fin(path_from_utf8(path), std::ios::binary);
    if (!fin) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: failed to open %s", kTag, path);
        return TRANSCRIBE_ERR_GGUF;
    }

    // ---- magic ----
    uint32_t magic = 0;
    if (!read_pod(fin, magic)) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: short read on magic", kTag);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (magic != k_whisper_bin_magic) {
        // Caller (transcribe_model_load_file) is responsible for
        // routing GGUF magic to the GGUF loader before reaching here.
        // Anything else with non-`ggml` magic is just unsupported.
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "%s: bad magic 0x%08x (expected 0x%08x for legacy "
                "whisper.cpp .bin)",
                kTag, magic, k_whisper_bin_magic);
        return TRANSCRIBE_ERR_GGUF;
    }

    // ---- hparams (11 × int32) ----
    auto & hp = out.hp;
    if (!read_pod(fin, hp.n_vocab) || !read_pod(fin, hp.n_audio_ctx) || !read_pod(fin, hp.n_audio_state) ||
        !read_pod(fin, hp.n_audio_head) || !read_pod(fin, hp.n_audio_layer) || !read_pod(fin, hp.n_text_ctx) ||
        !read_pod(fin, hp.n_text_state) || !read_pod(fin, hp.n_text_head) || !read_pod(fin, hp.n_text_layer) ||
        !read_pod(fin, hp.n_mels) || !read_pod(fin, hp.ftype)) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: short read on hparams", kTag);
        return TRANSCRIBE_ERR_GGUF;
    }

    if (!hparams_look_whisper(hp)) {
        // Magic was `ggml` but the geometry isn't Whisper. Likely a
        // Silero VAD `.bin` or some other ggml artifact — reject
        // cleanly so the caller surfaces UNSUPPORTED_ARCH.
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "%s: hparams do not match a known Whisper geometry "
                "(n_audio_layer=%d n_text_layer=%d n_mels=%d "
                "n_vocab=%d) — refusing to load as Whisper",
                kTag, hp.n_audio_layer, hp.n_text_layer, hp.n_mels, hp.n_vocab);
        return TRANSCRIBE_ERR_UNSUPPORTED_ARCH;
    }

    // is_multilingual / num_languages mirror the whisper.cpp
    // formulas at whisper.cpp:451-457. .en variants use n_vocab=51864.
    out.is_multilingual = hp.n_vocab >= 51865;
    out.num_languages   = hp.n_vocab - 51765 - (out.is_multilingual ? 1 : 0);

    // ---- mel filters ----
    if (!read_pod(fin, out.n_mel_filters) || !read_pod(fin, out.n_fft_filters)) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: short read on mel filter dims", kTag);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (out.n_mel_filters != hp.n_mels) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "%s: mel filter n_mel=%d disagrees with hparams "
                "n_mels=%d",
                kTag, out.n_mel_filters, hp.n_mels);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (out.n_fft_filters <= 0 || out.n_mel_filters <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: invalid mel filter dims n_mel=%d n_fft=%d", kTag, out.n_mel_filters,
                out.n_fft_filters);
        return TRANSCRIBE_ERR_GGUF;
    }
    // Whisper's frontend is fixed: n_fft=400 → n_fft/2+1=201 frequency
    // bins. Every shipped variant uses these exact dimensions, and the
    // adapter creates a [201, n_mels] tensor downstream — accepting any
    // other value here would mean a size-mismatched ggml_backend_tensor_set
    // later (partial write at best, OOB at worst).
    if (out.n_fft_filters != 201) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "%s: mel filter n_fft=%d is not 201 (whisper "
                "canonical n_fft=400 → n_fft/2+1=201); refusing "
                "to load",
                kTag, out.n_fft_filters);
        return TRANSCRIBE_ERR_GGUF;
    }
    const size_t fb_elems = static_cast<size_t>(out.n_mel_filters) * static_cast<size_t>(out.n_fft_filters);
    out.mel_filterbank.resize(fb_elems);
    fin.read(reinterpret_cast<char *>(out.mel_filterbank.data()),
             static_cast<std::streamsize>(fb_elems * sizeof(float)));
    if (!fin) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: short read on mel filterbank (%zu floats)", kTag, fb_elems);
        return TRANSCRIBE_ERR_GGUF;
    }

    // ---- vocab ----
    int32_t n_vocab_in_file = 0;
    if (!read_pod(fin, n_vocab_in_file)) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: short read on vocab count", kTag);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (n_vocab_in_file <= 0 || n_vocab_in_file > hp.n_vocab + 64) {
        // The file's vocab count may legitimately be smaller than
        // hparams.n_vocab (whisper.cpp synthesizes the missing tokens
        // at load), but it should never overshoot by much.
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: implausible vocab count %d (hparams n_vocab=%d)", kTag,
                n_vocab_in_file, hp.n_vocab);
        return TRANSCRIBE_ERR_GGUF;
    }
    out.vocab_tokens.reserve(static_cast<size_t>(n_vocab_in_file));
    std::string scratch;
    for (int32_t i = 0; i < n_vocab_in_file; ++i) {
        uint32_t len = 0;
        if (!read_pod(fin, len)) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: short read on vocab token %d length", kTag, i);
            return TRANSCRIBE_ERR_GGUF;
        }
        if (len > 1024) {
            // No legitimate Whisper token is anywhere near this long.
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: vocab token %d claims absurd length %u", kTag, i, len);
            return TRANSCRIBE_ERR_GGUF;
        }
        scratch.assign(static_cast<size_t>(len), '\0');
        if (len > 0) {
            fin.read(scratch.data(), static_cast<std::streamsize>(len));
            if (!fin) {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: short read on vocab token %d body", kTag, i);
                return TRANSCRIBE_ERR_GGUF;
            }
        }
        out.vocab_tokens.emplace_back(scratch);
    }

    // ---- tensors (header + dims + name + payload) ----
    // We walk the whole file, recording each tensor's metadata + the
    // file offset where its bytes start. We do NOT load payload bytes
    // here — stream_tensor_data_from_bin reopens the file fresh and
    // seeks per tensor, matching the GGUF byte-streaming pattern.
    while (true) {
        // Peek for EOF by attempting to read n_dims. If the read
        // fails cleanly at EOF (no partial bytes), we're done.
        int32_t n_dims = 0;
        fin.read(reinterpret_cast<char *>(&n_dims), sizeof(n_dims));
        if (fin.eof() && fin.gcount() == 0) {
            break;
        }
        if (!fin || fin.gcount() != sizeof(n_dims)) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: truncated tensor header (after %zu tensors)", kTag,
                    out.tensors.size());
            return TRANSCRIBE_ERR_GGUF;
        }

        int32_t name_len = 0;
        int32_t ttype    = 0;
        if (!read_pod(fin, name_len) || !read_pod(fin, ttype)) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: truncated tensor header (after %zu tensors)", kTag,
                    out.tensors.size());
            return TRANSCRIBE_ERR_GGUF;
        }
        if (n_dims < 1 || n_dims > 4) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: tensor n_dims=%d out of range", kTag, n_dims);
            return TRANSCRIBE_ERR_GGUF;
        }
        if (name_len <= 0 || name_len > 256) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: tensor name_len=%d out of range", kTag, name_len);
            return TRANSCRIBE_ERR_GGUF;
        }
        if (!is_known_ggml_type(ttype)) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: unknown ttype %d", kTag, ttype);
            return TRANSCRIBE_ERR_GGUF;
        }

        // Dimensions are stored in ne-order directly. The convert
        // script's "reverse the numpy shape" step is purely about
        // mapping numpy's row-major axis 0 (slowest) to ggml's ne[0]
        // (fastest); the bytes that follow are still C-order, and
        // whisper.cpp's loader reads dims into ne[i] without any
        // reversal (whisper.cpp:1884-1887). We do the same.
        BinTensorEntry entry{};
        entry.type   = static_cast<ggml_type>(ttype);
        entry.n_dims = n_dims;
        for (int32_t i = 0; i < 4; ++i) {
            entry.ne[i] = 1;
        }
        for (int32_t i = 0; i < n_dims; ++i) {
            int32_t v = 0;
            if (!read_pod(fin, v)) {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: truncated tensor dims", kTag);
                return TRANSCRIBE_ERR_GGUF;
            }
            if (v <= 0) {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: tensor dim ne[%d]=%d non-positive", kTag, i, v);
                return TRANSCRIBE_ERR_GGUF;
            }
            entry.ne[i] = v;
        }

        std::string name(static_cast<size_t>(name_len), '\0');
        fin.read(name.data(), name_len);
        if (!fin) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: truncated tensor name", kTag);
            return TRANSCRIBE_ERR_GGUF;
        }
        entry.name = std::move(name);

        // Payload: position is current; size is ggml_nbytes-equivalent
        // for the declared (type, ne).
        entry.nbytes = tensor_nbytes(entry.type, entry.ne);
        if (entry.nbytes == 0) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "%s: tensor \"%s\" has 0 nbytes (type=%s, "
                    "ne=[%lld,%lld,%lld,%lld] not a multiple of "
                    "block size)",
                    kTag, entry.name.c_str(), ggml_type_name(entry.type), (long long) entry.ne[0],
                    (long long) entry.ne[1], (long long) entry.ne[2], (long long) entry.ne[3]);
            return TRANSCRIBE_ERR_GGUF;
        }
        entry.offset = static_cast<uint64_t>(fin.tellg());

        // Skip the payload to land on the next tensor header.
        fin.seekg(static_cast<std::streamoff>(entry.nbytes), std::ios::cur);
        if (!fin) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "%s: tensor \"%s\" payload runs past end of file "
                    "(offset=%llu nbytes=%llu) — file is truncated",
                    kTag, entry.name.c_str(), (unsigned long long) entry.offset, (unsigned long long) entry.nbytes);
            return TRANSCRIBE_ERR_GGUF;
        }

        out.tensors.emplace_back(std::move(entry));
    }

    if (out.tensors.empty()) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: file declares no tensors", kTag);
        return TRANSCRIBE_ERR_GGUF;
    }

    return TRANSCRIBE_OK;
}

transcribe_status stream_tensor_data_from_bin(const std::string &                path,
                                              const std::vector<BinStreamSlot> & slots,
                                              const char *                       error_tag) {
    std::ifstream fin(path_from_utf8(path), std::ios::binary);
    if (!fin) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: failed to reopen %s for tensor data", error_tag, path.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }

    // Reused staging buffer: largest tensor in a quantized whisper
    // model is ~80 MB (token embedding); allocating once amortizes.
    std::vector<uint8_t> staging;

    for (const BinStreamSlot & s : slots) {
        if (s.dst == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: stream slot has null dst tensor", error_tag);
            return TRANSCRIBE_ERR_GGUF;
        }
        const size_t want = ggml_nbytes(s.dst);
        if (s.src_bytes != want) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "%s: tensor \"%s\" byte-size mismatch "
                    "(.bin says %llu, ggml expects %zu)",
                    error_tag, s.dst->name, (unsigned long long) s.src_bytes, want);
            return TRANSCRIBE_ERR_GGUF;
        }
        if (staging.size() < want) {
            staging.resize(want);
        }
        fin.seekg(static_cast<std::streamoff>(s.src_off));
        if (!fin) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: seek failed for tensor \"%s\"", error_tag, s.dst->name);
            return TRANSCRIBE_ERR_GGUF;
        }
        fin.read(reinterpret_cast<char *>(staging.data()), static_cast<std::streamsize>(want));
        if (!fin || static_cast<size_t>(fin.gcount()) != want) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: short read for tensor \"%s\"", error_tag, s.dst->name);
            return TRANSCRIBE_ERR_GGUF;
        }
        ggml_backend_tensor_set(s.dst, staging.data(), 0, want);
    }

    return TRANSCRIBE_OK;
}

}  // namespace transcribe::bin_loader
