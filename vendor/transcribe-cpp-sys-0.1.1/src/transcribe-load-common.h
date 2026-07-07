// transcribe-load-common.h - shared model-load scaffolding.
//
// INTERNAL, C++17. Not part of the public ABI.
//
// Every per-family load() walks the same three steps after building the
// GGUF catalog:
//
//   1. Resolve a BackendPlan from transcribe_model_load_params::backend.
//   2. Allocate a backend buffer for every tensor in ctx_meta on the plan's
//      primary backend.
//   3. Stream each tensor's bytes from the GGUF data section into its
//      backend slot via ggml_backend_tensor_set.
//
// These functions hoist that scaffolding; per-family load() still owns the
// model, params validation, weights catalog, fusion, and destructor wiring.

#pragma once

#include "transcribe-backend.h"
#include "transcribe.h"

#include <string>
#include <vector>

struct ggml_backend;
typedef struct ggml_backend * ggml_backend_t;
struct ggml_backend_buffer;
typedef struct ggml_backend_buffer * ggml_backend_buffer_t;
struct ggml_context;
struct ggml_tensor;
struct gguf_context;

namespace transcribe::load_common {

// Resolve a BackendPlan from a caller's backend request by walking
// ggml's device registry.
//
//   requested:   the caller's transcribe_backend_request.
//                  AUTO   - best available: GPU/IGPU → ACCEL → CPU.
//                  CPU    - strict CPU only. No GPU, no ACCEL.
//                  METAL  - require Metal; error if not present.
//                  VULKAN - require Vulkan; error if not present.
//                Library-internal classification of what each value
//                actually picks up is documented in
//                transcribe-backend.h.
//   gpu_device:  multi-GPU selector. 0 (the default) means "auto / the
//                first device of the chosen kind" — the existing
//                first-of-kind behavior. A value > 0 selects the GPU/IGPU
//                device at that global ggml registry index (the same index
//                space transcribe_get_backend_device() enumerates) as the
//                primary, validated against `requested`: the device must be
//                a GPU/IGPU, and for a specific METAL/VULKAN/CUDA request it
//                must be that vendor. gpu_device is not valid for a
//                CPU / CPU_ACCEL request (there is no GPU to pick) nor
//                negative; both return TRANSCRIBE_ERR_INVALID_ARG.
//   error_tag:   log prefix, e.g. "parakeet".
//   out:         populated on success. out.primary is the backend
//                that owns the weight buffer; out.scheduler_list
//                always has at least the primary and typically has
//                CPU last as a fallback. See BackendPlan for the
//                full semantic.
//
// Returns:
//   TRANSCRIBE_OK         on success.
//   TRANSCRIBE_ERR_INVALID_ARG if gpu_device is negative, out of range,
//                          names a non-GPU device, names a device whose
//                          vendor doesn't match a specific GPU request, or
//                          is non-zero for a CPU request.
//   TRANSCRIBE_ERR_BACKEND if the caller asked for a specific
//                          backend (METAL / VULKAN) that could not
//                          be initialized, or if the CPU backend
//                          itself fails to initialize (there is no
//                          fallback past CPU).
transcribe_status init_backends(transcribe_backend_request requested,
                                int                        gpu_device,
                                const char *               error_tag,
                                BackendPlan &              out);

// Stream every tensor in `ctx_meta` from the GGUF data section on
// disk into its already-allocated backend buffer slot via
// ggml_backend_tensor_set. The backend buffer must already be
// allocated (typically via ggml_backend_alloc_ctx_tensors) before
// calling this.
//
//   path:      filesystem path to the GGUF file, reopened fresh via
//              std::ifstream. Stable for the life of the call.
//   gguf_data: the full gguf_context from gguf_init_from_file with
//              no_alloc=true. Used to resolve each tensor's data
//              offset.
//   ctx_meta:  the ggml_context that built_*_weights() populated
//              (tensors with backend-bound data slots).
//   error_tag: log prefix for diagnostics.
//
// Returns TRANSCRIBE_OK on success, TRANSCRIBE_ERR_GGUF on any
// I/O or tensor-not-found failure. On failure ctx_meta's tensors
// may be partially populated; the caller should discard the
// whole model state on error.
transcribe_status stream_tensor_data(const std::string &  path,
                                     const gguf_context * gguf_data,
                                     ggml_context *       ctx_meta,
                                     const char *         error_tag);

// A single F16 → F32 promotion target: `dst_slot` is a pointer into a
// family weights struct (e.g. &block.conv_pw1_w) that currently holds
// the F16 source tensor and will be repointed to the F32 replacement.
struct ConvPwF32Slot {
    ggml_tensor ** dst_slot;
    ggml_tensor *  src;
};

// Dequantize a list of pointwise-conv weights from F16 to F32 when the
// primary backend is CPU. Used by conformer families (parakeet, cohere)
// whose encoder conv modules do a conv1d followed by a 1x1 pointwise
// conv; with F16 weights, the 1x1 path is bottlenecked by per-element
// dequantize inside matmul. Promoting the pointwise weights to F32 at
// load time eliminates that hot spot for the CPU backend only.
//
// Ownership on success: the caller receives a new ggml_context and
// backend buffer (written via *out_ctx / *out_buffer) that hold the F32
// replacements. Each slot's *dst_slot is repointed to the matching F32
// tensor. The original F16 tensors remain live in their original buffer
// (ggml's backend buffer model does not support freeing individual
// tensors); the cost is ~235 MB for cohere, much less for parakeet.
//
// Behavior:
//   - If plan.primary_kind != BackendKind::Cpu: no-op, returns
//     TRANSCRIBE_OK; *out_ctx and *out_buffer are NOT touched. This is
//     the important semantic: strict-CPU is keyed off the classified
//     kind, not off ACCEL-vs-CPU ordering in a backend list, so a
//     request for TRANSCRIBE_BACKEND_CPU reliably triggers promotion
//     even on machines where ACCEL would otherwise sort ahead of CPU.
//   - If `slots` is empty: no-op, same return semantics.
//   - If the F16 → float type trait is missing: logs a warning and
//     returns TRANSCRIBE_OK without modifying anything.
//
// error_tag: log prefix, e.g. "parakeet" or "cohere".
transcribe_status promote_conv_pw_f16_to_f32_on_cpu(const BackendPlan &                plan,
                                                    const std::vector<ConvPwF32Slot> & slots,
                                                    const char *                       error_tag,
                                                    ggml_context **                    out_ctx,
                                                    ggml_backend_buffer_t *            out_buffer);

// Read a small F32 tensor from the GGUF file into a std::vector<float>.
//
// Validates:
//   - tensor exists in the GGUF index (returns Absent if not)
//   - tensor type is GGML_TYPE_F32 (returns BadType if not)
//   - byte count is divisible by sizeof(float)
//   - byte count matches expected_elems * sizeof(float) when
//     expected_elems > 0 (pass 0 to skip the size check)
//   - ifstream read consumed exactly the right number of bytes
//
// On success, `out` is resized and filled. On every other return
// (including Absent), `out` is cleared so stale data cannot leak.
// On any failure other than Absent, a diagnostic is logged using
// `error_tag` and `tensor_name`.
enum class ReadF32Result {
    Ok,       // tensor found, validated, and read successfully
    Absent,   // tensor not in the GGUF index (caller should compute)
    BadType,  // tensor exists but is not F32
    BadSize,  // tensor exists but byte count is wrong
    ReadErr,  // I/O error during read
};

ReadF32Result read_f32_tensor_checked(gguf_context *       gguf_ctx,
                                      const std::string &  gguf_path,
                                      const char *         tensor_name,
                                      size_t               expected_elems,  // 0 = skip size check
                                      const char *         error_tag,
                                      std::vector<float> & out);

}  // namespace transcribe::load_common
