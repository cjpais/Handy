// transcribe-bin-loader.h - parser for the legacy whisper.cpp `.bin`
// weight format.
//
// INTERNAL. C++17. Consumed only from other .cpp files inside src/.
// Not part of the public include/transcribe.h ABI.
//
// Scope: this module knows the byte layout of a whisper.cpp `.bin` and
// nothing else. It does not allocate ggml tensors, does not know
// anything about WhisperHParams / WhisperWeights, and does not load
// payload bytes into memory. The output is a metadata-only manifest
// (header, mel filters, vocab, plus per-tensor (offset, nbytes) entries)
// that the per-arch adapter — currently only whisper — turns into a
// canonical model state.
//
// Why metadata-only: a quantized large-v3 .bin is ~1.5 GB. Holding all
// payload bytes in std::vector at parse time would double peak memory.
// The byte-streaming step that comes later (stream_tensor_data_from_bin)
// reopens the file fresh and seeks per tensor, the same way the GGUF
// path does.
//
// Layout (whisper.cpp's monolithic format — single magic, no version):
//
//   magic            uint32  0x67676d6c ("ggml" little-endian)
//   hparams          11 × int32 (n_vocab, n_audio_*, n_text_*, n_mels, ftype)
//   mel filters      int32 n_mel, int32 n_fft, then n_mel*n_fft float32s
//   vocab            int32 count, then each token: int32 len + raw bytes
//   tensors (×N)     int32 n_dims, int32 name_len, int32 ttype,
//                    n_dims × int32 dims (in ggml ne order: ne[0] first,
//                    fastest-varying; the convert script writes the
//                    reverse of numpy shape, which is exactly ne order),
//                    name_len bytes name, then ggml_nbytes bytes payload.

#pragma once

#include "ggml.h"
#include "transcribe.h"

#include <cstdint>
#include <string>
#include <vector>

namespace transcribe::bin_loader {

// Whisper .bin magic, "ggml" in little-endian (see whisper.cpp ggml.h).
inline constexpr uint32_t k_whisper_bin_magic = 0x67676d6cu;

// The 11 int32 hparams in the order the file stores them.
struct WhisperBinHParams {
    int32_t n_vocab       = 0;
    int32_t n_audio_ctx   = 0;
    int32_t n_audio_state = 0;
    int32_t n_audio_head  = 0;
    int32_t n_audio_layer = 0;
    int32_t n_text_ctx    = 0;
    int32_t n_text_state  = 0;
    int32_t n_text_head   = 0;
    int32_t n_text_layer  = 0;
    int32_t n_mels        = 0;
    int32_t ftype         = 0;  // raw value; ftype % 1000 = ggml_type
};

// Per-tensor metadata. `offset` is from the start of the file (not
// from any data section). `nbytes` equals ggml_nbytes(...) for the
// declared type + ne — validated at parse time so the streamer can
// trust both fields.
struct BinTensorEntry {
    std::string name;  // legacy whisper.cpp tensor name
    ggml_type   type   = GGML_TYPE_F32;
    int32_t     n_dims = 0;
    int64_t     ne[4]  = { 1, 1, 1, 1 };  // ggml convention (file-reversed)
    uint64_t    offset = 0;
    uint64_t    nbytes = 0;
};

// Whole-file parse result, sans tensor payload bytes.
struct WhisperBinModel {
    std::string       path;
    WhisperBinHParams hp;
    bool              is_multilingual = false;
    int32_t           num_languages   = 0;

    int32_t            n_mel_filters = 0;   // n_mel from the file
    int32_t            n_fft_filters = 0;   // n_fft/2+1
    std::vector<float> mel_filterbank;      // size n_mel * n_fft

    std::vector<std::string> vocab_tokens;  // raw byte strings

    std::vector<BinTensorEntry> tensors;
};

// Parse + validate a whisper.cpp legacy `.bin`.
//
// Return values:
//   TRANSCRIBE_OK                   - magic + hparams whisper-shaped,
//                                     mel filters / vocab / tensor
//                                     manifest all internally consistent.
//   TRANSCRIBE_ERR_FILE_NOT_FOUND   - path does not exist.
//   TRANSCRIBE_ERR_UNSUPPORTED_ARCH - magic is `ggml` but the hparams
//                                     don't match a known Whisper
//                                     geometry. Used to reject non-
//                                     Whisper `ggml`-magic files like
//                                     Silero VAD without crashing.
//   TRANSCRIBE_ERR_GGUF             - structural failure (wrong magic,
//                                     truncated header, unknown ttype,
//                                     declared payload doesn't match
//                                     ggml_nbytes, vocab/tensor lengths
//                                     run past EOF, …).
//
// The `for-tests-*.bin` fixtures in the upstream models/ tree pass the
// hparams gate (real Whisper geometry, just truncated). They fail at
// the tensor-payload completeness check inside the manifest pass and
// surface as ERR_GGUF with a specific "truncated" diagnostic.
transcribe_status parse_whisper_bin(const char * path, WhisperBinModel & out);

// Stream every tensor in `ctx_meta` (which the caller has already built
// with canonical names + canonical ne + the source's ggml_type, then
// allocated to backend memory) from the `.bin` on disk into the
// allocated slot via ggml_backend_tensor_set.
//
// The caller must pass a parallel vector pairing each ctx_meta tensor
// with its source byte range — see whisper::load_from_bin for the
// construction. (We do not store this map inside WhisperBinModel
// because the canonical-name / squeeze rules are arch-specific.)
struct BinStreamSlot {
    ggml_tensor * dst       = nullptr;
    uint64_t      src_off   = 0;
    uint64_t      src_bytes = 0;
};

transcribe_status stream_tensor_data_from_bin(const std::string &                path,
                                              const std::vector<BinStreamSlot> & slots,
                                              const char *                       error_tag);

}  // namespace transcribe::bin_loader
