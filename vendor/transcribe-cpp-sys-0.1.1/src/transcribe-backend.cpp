// transcribe-backend.cpp - internal backend selection helpers.
//
// See transcribe-backend.h for rationale. This file owns the
// device-classification rules: given a ggml_backend_dev_t, what
// library-level BackendKind does it correspond to?

#include "transcribe-backend.h"

#include "transcribe-log.h"

#include <cstdlib>
#include <cstring>
#include <stdexcept>

namespace transcribe {

const char * kind_name(BackendKind kind) {
    switch (kind) {
        case BackendKind::Cpu:
            return "cpu";
        case BackendKind::Metal:
            return "metal";
        case BackendKind::Vulkan:
            return "vulkan";
        case BackendKind::Cuda:
            return "cuda";
        case BackendKind::Sycl:
            return "sycl";
        case BackendKind::Accel:
            return "accel";
        case BackendKind::OtherGpu:
            return "gpu";
        case BackendKind::Unknown:
        default:
            return "unknown";
    }
}

// Return true if `reg_name` (the ggml backend registry name) starts
// with the given prefix. ggml's registry names look like "MTL",
// "Vulkan", "CUDA", "SYCL", "BLAS", "CPU", etc. Prefix matching is
// intentional: registry names can get version suffixes or device
// index suffixes in some ggml builds.
static bool reg_name_is(const char * reg_name, const char * prefix) {
    if (reg_name == nullptr || prefix == nullptr) {
        return false;
    }
    return std::strncmp(reg_name, prefix, std::strlen(prefix)) == 0;
}

BackendKind classify_backend_type(enum ggml_backend_dev_type dev_type, const char * reg_name) {
    if (dev_type == GGML_BACKEND_DEVICE_TYPE_CPU) {
        return BackendKind::Cpu;
    }
    if (dev_type == GGML_BACKEND_DEVICE_TYPE_ACCEL) {
        return BackendKind::Accel;
    }
    if (dev_type != GGML_BACKEND_DEVICE_TYPE_GPU && dev_type != GGML_BACKEND_DEVICE_TYPE_IGPU) {
        return BackendKind::Unknown;
    }

    if (reg_name_is(reg_name, "MTL") || reg_name_is(reg_name, "Metal")) {
        return BackendKind::Metal;
    } else if (reg_name_is(reg_name, "Vulkan")) {
        return BackendKind::Vulkan;
    } else if (reg_name_is(reg_name, "CUDA")) {
        return BackendKind::Cuda;
    } else if (reg_name_is(reg_name, "SYCL")) {
        return BackendKind::Sycl;
    }

    return BackendKind::OtherGpu;
}

BackendKind classify_device(ggml_backend_dev_t dev) {
    if (dev == nullptr) {
        return BackendKind::Unknown;
    }

    // First cut: ggml's device-type classification. This tells us CPU
    // vs GPU vs IGPU vs ACCEL without any name matching. For GPU and
    // IGPU devices, the registry name resolves the vendor-specific kind.
    const auto         dev_type = ggml_backend_dev_type(dev);
    ggml_backend_reg_t reg      = ggml_backend_dev_backend_reg(dev);
    const char *       reg_name = (reg != nullptr) ? ggml_backend_reg_name(reg) : nullptr;
    return classify_backend_type(dev_type, reg_name);
}

namespace {

// Shared body for safe_* teardown wrappers. The test hook fires after the
// real free; present-but-empty is inert.
template <typename Fn> void contained_free(const char * what, Fn && do_free) noexcept {
    try {
        do_free();
        if (const char * hook = std::getenv("TRANSCRIBE_TEST_TEARDOWN_THROW"); hook != nullptr && hook[0] != '\0') {
            throw std::runtime_error("TRANSCRIBE_TEST_TEARDOWN_THROW fault injection");
        }
    } catch (const std::exception & e) {
        log_msg(TRANSCRIBE_LOG_LEVEL_WARN, "%s threw during teardown (contained; resource may leak): %s", what,
                e.what());
    } catch (...) {
        log_msg(TRANSCRIBE_LOG_LEVEL_WARN,
                "%s threw an unknown exception during teardown (contained; resource may leak)", what);
    }
}

}  // namespace

void safe_backend_free(ggml_backend_t backend) noexcept {
    if (backend == nullptr) {
        return;
    }
    contained_free("ggml_backend_free", [&] { ggml_backend_free(backend); });
}

void safe_buffer_free(ggml_backend_buffer_t buffer) noexcept {
    if (buffer == nullptr) {
        return;
    }
    contained_free("ggml_backend_buffer_free", [&] { ggml_backend_buffer_free(buffer); });
}

void safe_sched_free(ggml_backend_sched_t sched) noexcept {
    if (sched == nullptr) {
        return;
    }
    contained_free("ggml_backend_sched_free", [&] { ggml_backend_sched_free(sched); });
}

}  // namespace transcribe
