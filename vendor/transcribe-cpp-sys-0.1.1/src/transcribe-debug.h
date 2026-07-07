// transcribe-debug.h - per-stage tensor dumping for the numerical
// accuracy harness.
//
// Writes a named tensor to disk in the format scripts/dump_reference_*.py
// emits, so scripts/compare_tensors.py can diff C++ vs Python dump dirs
// directly.
//
// On-disk format (matches dump_reference_*.py::write_dump):
//   <dump_dir>/<name>.f32   raw little-endian fp32, row-major (C order)
//   <dump_dir>/<name>.json  sidecar: { name, stage?, shape (slow-to-fast),
//                           dtype, layout, min, max, mean, source }
//
// Critical layout note: ggml ne[] is fast-to-slow (ne[0] = innermost).
// Numpy / the Python dumpers use slow-to-fast. The dumper reverses ne[]
// and drops trailing 1s, so ggml ne=[1024, 275, 1, 1] ([d_model, T, B=1])
// lands on disk as shape=[275, 1024] — the reference's [T, d_model] after
// squeezing the batch dim.
//
// Gated on TRANSCRIBE_DUMP_DIR: unset → init() is a no-op and dumps return
// immediately. Not thread-safe (file writes are unsynchronized); single-
// threaded use only.

#pragma once

struct ggml_tensor;

namespace transcribe::debug {

// Initialize the debug dumper from environment. Reads
// TRANSCRIBE_DUMP_DIR. If unset or empty, the dumper stays disabled
// and dump_tensor() is a no-op. Idempotent: safe to call multiple
// times in one process; only the first call has effect. Returns true
// if the dumper is enabled after this call.
bool init();

// True if the dumper has been init()'d AND TRANSCRIBE_DUMP_DIR was
// set. Cheap enough to gate hot-path code paths on.
bool enabled();

// The directory the dumper is writing into, or nullptr if disabled.
// Lifetime is the process; the returned pointer stays valid as long
// as the dumper is enabled. Useful for tests.
const char * dump_dir();

// Validation-hook gating ------------------------------------------------------
//
// The reference-mel injection (TRANSCRIBE_MEL_FROM_REF) and the per-layer
// tensor dumps (TRANSCRIBE_DUMP_ALL_BLOCKS / TRANSCRIBE_DUMP_SUB_BLOCKS) are
// numerical-parity hooks for porting/validation. They are compiled in only when
// the library is built with -DTRANSCRIBE_ENABLE_VALIDATION_HOOKS=ON (see the
// CMake option). Release builds leave them out.

// True if the library was compiled with validation hooks enabled.
bool validation_hooks_enabled();

// Per-layer block dump opt-in (TRANSCRIBE_DUMP_ALL_BLOCKS). Always false unless
// the library was built with validation hooks. The env var name lives only in
// transcribe-debug.cpp (behind the compile guard) so it is absent from the
// strings of a release binary.
bool dump_all_blocks_requested();

// TRANSCRIBE_DUMP_SUB_BLOCKS spec (CSV of block indices to dump sub-layer
// activations for) or nullptr. Always nullptr unless built with validation
// hooks.
const char * dump_sub_blocks_spec();

// Push/pop a name prefix that gets prepended to every subsequent
// dump_tensor / dump_host_f32 call. Used by buffered streaming to
// scope per-chunk intermediate dumps (e.g. "stream.chunk.5.") so the
// encoder's `enc.block.<L>.out` tensors don't get overwritten across
// chunks. Stack-style — last push wins. Empty string clears the
// prefix. No-op when the dumper is disabled.
void push_name_prefix(const char * prefix);
void pop_name_prefix();

// Preserve a ggml tensor for a later dump_tensor() call.
//
// The ggml scheduler is allowed to reuse intermediate buffers unless a
// tensor is marked as a graph output. Family graph builders that stash
// intermediate tensor pointers in an EncoderDumps-style struct must call
// this helper while building the graph, before scheduler allocation.
// Otherwise dump_tensor() may read a reused buffer after graph_compute.
//
// No-op unless debug dumping is enabled, so normal inference keeps the
// scheduler's live-range packing unchanged.
void mark_tensor_for_dump(struct ggml_tensor * tensor);

// Dump a tensor to <dump_dir>/<name>.{f32,json}.
//
// `tensor` may live on any backend; the dumper copies the bytes via
// ggml_backend_tensor_get, which is the universal API for reading
// tensor data regardless of backend (host buffer ⇒ memcpy; discrete
// GPU ⇒ readback).
//
// `name` becomes the filename stem. It must be a non-empty string
// containing only filesystem-safe characters (no '/' or '\').
//
// `stage` is an optional provenance label written into the JSON
// sidecar; pass nullptr to omit.
//
// On any failure (disabled, null tensor, dtype other than fp32, IO
// error, malloc error), logs to stderr and returns without throwing.
// Dumping is debug-only and must never affect compute correctness or
// propagate errors out of a graph build.
void dump_tensor(const char * name, const struct ggml_tensor * tensor, const char * stage = nullptr);

// Dump a host-side fp32 buffer to <dump_dir>/<name>.{f32,json}.
//
// Used by code paths that don't run on a ggml backend — e.g. a decoder
// that runs the LSTM + joint + TDT loop directly on host floats.
//
// `data` points at `n_elem` contiguous fp32 values. `shape` is the
// numpy/row-major (slow-to-fast) shape and must satisfy
// product(shape) == n_elem; pass `{n_elem}` for a 1D vector.
//
// Same name / stage / failure semantics as the ggml-tensor variant.
void dump_host_f32(const char *      name,
                   const float *     data,
                   long long         n_elem,
                   const long long * shape,
                   int               n_dims,
                   const char *      stage = nullptr);

}  // namespace transcribe::debug
