// transcribe-debug.cpp - implementation of the per-stage tensor dumper.
//
// See transcribe-debug.h for the public API and the on-disk format
// contract. The implementation is intentionally dependency-light: no
// JSON library, no fmt, no exceptions across the boundary. The only
// runtime cost is the env var read at init() time and the per-call
// device→host copy + file writes (gated on TRANSCRIBE_DUMP_DIR).

#include "transcribe-debug.h"

#include "ggml-backend.h"
#include "ggml.h"
#include "transcribe-env.h"

#include <cmath>
#include <cstdarg>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <ios>
#include <limits>
#include <new>
#include <string>
#include <vector>

namespace transcribe::debug {

namespace {

// One-shot init state. The first init() call captures the env var
// and decides whether the dumper is enabled. Subsequent calls are
// no-ops. Not thread-safe (per the contract in the header).
bool        g_initialized = false;
bool        g_enabled     = false;
std::string g_dump_dir;

// Name prefix stack. push_name_prefix appends, pop_name_prefix removes
// the last. The effective prefix is the top of the stack. Used to
// scope per-chunk intermediate dumps in buffered streaming.
std::vector<std::string> g_name_prefix_stack;

std::string make_prefixed_name(const char * name) {
    if (g_name_prefix_stack.empty()) {
        return std::string(name);
    }
    return g_name_prefix_stack.back() + name;
}

// Reject filenames containing path separators or other characters
// that would let a caller escape the dump dir. The encoder will
// generate names like "enc.pre_encode.out" or "enc.block.0.attn" —
// dots and digits are fine, slashes are not.
bool name_is_safe(const char * name) {
    if (name == nullptr || name[0] == '\0') {
        return false;
    }
    for (const char * p = name; *p; ++p) {
        const char c = *p;
        if (c == '/' || c == '\\') {
            return false;
        }
        // Reject non-printable / control characters defensively.
        if (static_cast<unsigned char>(c) < 0x20) {
            return false;
        }
    }
    return true;
}

// Compute the numpy/row-major shape of a ggml tensor by reversing
// ne[] and dropping trailing 1s. The result is the slow-to-fast
// shape that matches the Python reference dumpers' `data.shape` from
// numpy.
//
// Examples:
//   ggml ne = [3, 5, 1, 1]    -> shape = [5, 3]
//   ggml ne = [1024, 275, 1, 1] -> shape = [275, 1024]
//   ggml ne = [128, 1101, 1, 1] -> shape = [1101, 128]
//   ggml ne = [4, 4, 4, 4]    -> shape = [4, 4, 4, 4]
std::vector<int64_t> row_major_shape(const ggml_tensor * t) {
    std::vector<int64_t> out;
    int                  last = GGML_MAX_DIMS - 1;
    while (last > 0 && t->ne[last] == 1) {
        --last;
    }
    out.reserve(static_cast<size_t>(last + 1));
    for (int i = last; i >= 0; --i) {
        out.push_back(t->ne[i]);
    }
    return out;
}

// Write a float value that is safe for JSON. Inf and NaN are not valid
// JSON numbers, so we output null for those.
void write_json_float(std::ofstream & js, double v) {
    if (std::isnan(v) || std::isinf(v)) {
        js << "null";
    } else {
        js << v;
    }
}

void warn(const char * fmt, ...) {
    std::va_list ap;
    va_start(ap, fmt);
    std::fprintf(stderr, "transcribe-debug: ");
    std::vfprintf(stderr, fmt, ap);
    std::fprintf(stderr, "\n");
    va_end(ap);
}

}  // namespace

bool init() {
    if (g_initialized) {
        return g_enabled;
    }
    g_initialized = true;
    g_enabled     = false;

    const char * env = std::getenv("TRANSCRIBE_DUMP_DIR");
    if (env == nullptr || env[0] == '\0') {
        return false;
    }
    g_dump_dir = env;
    g_enabled  = true;
    std::fprintf(stderr, "transcribe-debug: dumping tensors to %s\n", g_dump_dir.c_str());
    return true;
}

bool enabled() {
    return g_initialized && g_enabled;
}

const char * dump_dir() {
    return enabled() ? g_dump_dir.c_str() : nullptr;
}

bool validation_hooks_enabled() {
#ifdef TRANSCRIBE_ENABLE_VALIDATION_HOOKS
    return true;
#else
    return false;
#endif
}

bool dump_all_blocks_requested() {
#ifdef TRANSCRIBE_ENABLE_VALIDATION_HOOKS
    return transcribe::env::flag("TRANSCRIBE_DUMP_ALL_BLOCKS");
#else
    return false;
#endif
}

const char * dump_sub_blocks_spec() {
#ifdef TRANSCRIBE_ENABLE_VALIDATION_HOOKS
    return transcribe::env::str("TRANSCRIBE_DUMP_SUB_BLOCKS");
#else
    return nullptr;
#endif
}

void push_name_prefix(const char * prefix) {
    if (!enabled() || prefix == nullptr) {
        return;
    }
    g_name_prefix_stack.emplace_back(prefix);
}

void pop_name_prefix() {
    if (!enabled()) {
        return;
    }
    if (!g_name_prefix_stack.empty()) {
        g_name_prefix_stack.pop_back();
    }
}

void mark_tensor_for_dump(ggml_tensor * tensor) {
    if (enabled() && tensor != nullptr) {
        ggml_set_output(tensor);
    }
}

void dump_tensor(const char * name, const ggml_tensor * tensor, const char * stage) {
    if (!enabled()) {
        return;
    }
    if (tensor == nullptr) {
        warn("dump_tensor(\"%s\"): null tensor", name ? name : "(null)");
        return;
    }
    if (!name_is_safe(name)) {
        warn("dump_tensor: rejecting unsafe name \"%s\"", name ? name : "(null)");
        return;
    }
    if (tensor->type != GGML_TYPE_F32) {
        warn("dump_tensor(\"%s\"): only fp32 supported, got %s", name, ggml_type_name(tensor->type));
        return;
    }

    // Bytes to copy. ggml_nbytes accounts for non-contiguous tensors
    // by walking nb[]; for the encoder's outputs we expect them all
    // to be contiguous (a fresh ggml_new_tensor or the result of a
    // ggml op writing into a freshly-allocated buffer), but the
    // copy below works either way because ggml_backend_tensor_get
    // operates on the dense byte range starting at the tensor's
    // offset.
    const size_t nbytes = ggml_nbytes(tensor);
    if (nbytes == 0) {
        warn("dump_tensor(\"%s\"): tensor has 0 bytes", name);
        return;
    }
    if (nbytes % sizeof(float) != 0) {
        warn("dump_tensor(\"%s\"): nbytes (%zu) not a multiple of sizeof(float)", name, nbytes);
        return;
    }

    // Device → host copy. ggml_backend_tensor_get is the universal
    // API: on host buffers it's a memcpy, on discrete GPUs it's a
    // readback.
    std::vector<uint8_t> host;
    try {
        host.resize(nbytes);
    } catch (const std::bad_alloc &) {
        warn("dump_tensor(\"%s\"): malloc failed for %zu bytes", name, nbytes);
        return;
    }
    ggml_backend_tensor_get(tensor, host.data(), 0, nbytes);

    // Per-element stats for the JSON sidecar. These are eyeballing
    // aids — the actual numerical comparison is byte-level via the
    // .f32 file. Computed in fp64 to avoid catastrophic cancellation
    // on the mean.
    const size_t  n_elem = nbytes / sizeof(float);
    const float * fdata  = reinterpret_cast<const float *>(host.data());
    float         vmin   = std::numeric_limits<float>::infinity();
    float         vmax   = -std::numeric_limits<float>::infinity();
    double        vsum   = 0.0;
    for (size_t i = 0; i < n_elem; ++i) {
        const float v = fdata[i];
        if (v < vmin) {
            vmin = v;
        }
        if (v > vmax) {
            vmax = v;
        }
        vsum += static_cast<double>(v);
    }
    const double vmean = vsum / static_cast<double>(n_elem);

    // Build paths. Honors the active name prefix (push_name_prefix /
    // pop_name_prefix) so callers can scope dumps without rewriting
    // tensor names in the graph builders.
    const std::string full_name = make_prefixed_name(name);
    const std::string f32_path  = g_dump_dir + "/" + full_name + ".f32";
    const std::string json_path = g_dump_dir + "/" + full_name + ".json";

    // Write the .f32 first; if that fails, don't bother with the
    // sidecar (an unpaired sidecar is more confusing than no dump
    // at all).
    {
        std::ofstream f32(f32_path, std::ios::binary | std::ios::trunc);
        if (!f32) {
            warn("dump_tensor(\"%s\"): cannot open %s for write", name, f32_path.c_str());
            return;
        }
        f32.write(reinterpret_cast<const char *>(host.data()), static_cast<std::streamsize>(nbytes));
        if (!f32) {
            warn("dump_tensor(\"%s\"): write failed for %s", name, f32_path.c_str());
            return;
        }
    }

    // Write the JSON sidecar. Hand-rolled formatter — the schema is
    // tiny and stable, no need for a JSON library.
    {
        std::ofstream js(json_path, std::ios::trunc);
        if (!js) {
            warn("dump_tensor(\"%s\"): cannot open %s for write", name, json_path.c_str());
            return;
        }
        const std::vector<int64_t> shape = row_major_shape(tensor);

        js << "{\n";
        js << "  \"name\": \"" << name << "\",\n";
        if (stage != nullptr && stage[0] != '\0') {
            js << "  \"stage\": \"" << stage << "\",\n";
        }
        js << "  \"shape\": [";
        for (size_t i = 0; i < shape.size(); ++i) {
            if (i) {
                js << ", ";
            }
            js << shape[i];
        }
        js << "],\n";
        js << "  \"dtype\": \"f32\",\n";
        js << "  \"layout\": \"row-major\",\n";
        // Use std::scientific with enough precision that any nonzero
        // value round-trips visibly. The .f32 file is the
        // bit-precise source of truth; these are for humans.
        js.precision(9);
        js << "  \"min\": ";
        write_json_float(js, vmin);
        js << ",\n";
        js << "  \"max\": ";
        write_json_float(js, vmax);
        js << ",\n";
        js << "  \"mean\": ";
        write_json_float(js, vmean);
        js << ",\n";
        js << "  \"source\": { \"kind\": \"cpp\" }\n";
        js << "}\n";
        if (!js) {
            warn("dump_tensor(\"%s\"): write failed for %s", name, json_path.c_str());
            return;
        }
    }
}

void dump_host_f32(const char *      name,
                   const float *     data,
                   long long         n_elem,
                   const long long * shape,
                   int               n_dims,
                   const char *      stage) {
    if (!enabled()) {
        return;
    }
    if (data == nullptr || n_elem <= 0) {
        warn("dump_host_f32(\"%s\"): null data or zero elements", name ? name : "(null)");
        return;
    }
    if (!name_is_safe(name)) {
        warn("dump_host_f32: rejecting unsafe name \"%s\"", name ? name : "(null)");
        return;
    }
    if (shape == nullptr || n_dims <= 0) {
        warn("dump_host_f32(\"%s\"): missing shape", name);
        return;
    }
    long long shape_product = 1;
    for (int i = 0; i < n_dims; ++i) {
        if (shape[i] <= 0) {
            warn("dump_host_f32(\"%s\"): non-positive shape[%d]=%lld", name, i, shape[i]);
            return;
        }
        shape_product *= shape[i];
    }
    if (shape_product != n_elem) {
        warn("dump_host_f32(\"%s\"): shape product %lld != n_elem %lld", name, shape_product, n_elem);
        return;
    }

    const size_t nbytes = static_cast<size_t>(n_elem) * sizeof(float);

    // Stats for the JSON sidecar.
    float  vmin = std::numeric_limits<float>::infinity();
    float  vmax = -std::numeric_limits<float>::infinity();
    double vsum = 0.0;
    for (long long i = 0; i < n_elem; ++i) {
        const float v = data[i];
        if (v < vmin) {
            vmin = v;
        }
        if (v > vmax) {
            vmax = v;
        }
        vsum += static_cast<double>(v);
    }
    const double vmean = vsum / static_cast<double>(n_elem);

    const std::string full_name = make_prefixed_name(name);
    const std::string f32_path  = g_dump_dir + "/" + full_name + ".f32";
    const std::string json_path = g_dump_dir + "/" + full_name + ".json";

    {
        std::ofstream f32(f32_path, std::ios::binary | std::ios::trunc);
        if (!f32) {
            warn("dump_host_f32(\"%s\"): cannot open %s for write", name, f32_path.c_str());
            return;
        }
        f32.write(reinterpret_cast<const char *>(data), static_cast<std::streamsize>(nbytes));
        if (!f32) {
            warn("dump_host_f32(\"%s\"): write failed for %s", name, f32_path.c_str());
            return;
        }
    }

    {
        std::ofstream js(json_path, std::ios::trunc);
        if (!js) {
            warn("dump_host_f32(\"%s\"): cannot open %s for write", name, json_path.c_str());
            return;
        }
        js << "{\n";
        js << "  \"name\": \"" << full_name << "\",\n";
        if (stage != nullptr && stage[0] != '\0') {
            js << "  \"stage\": \"" << stage << "\",\n";
        }
        js << "  \"shape\": [";
        for (int i = 0; i < n_dims; ++i) {
            if (i) {
                js << ", ";
            }
            js << shape[i];
        }
        js << "],\n";
        js << "  \"dtype\": \"f32\",\n";
        js << "  \"layout\": \"row-major\",\n";
        js.precision(9);
        js << "  \"min\": ";
        write_json_float(js, vmin);
        js << ",\n";
        js << "  \"max\": ";
        write_json_float(js, vmax);
        js << ",\n";
        js << "  \"mean\": ";
        write_json_float(js, vmean);
        js << ",\n";
        js << "  \"source\": { \"kind\": \"cpp\" }\n";
        js << "}\n";
        if (!js) {
            warn("dump_host_f32(\"%s\"): write failed for %s", name, json_path.c_str());
            return;
        }
    }
}

}  // namespace transcribe::debug
