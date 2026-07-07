cmake_minimum_required(VERSION 3.16)

# -----------------------------------------------------------------------------
# Version (single source of truth: include/transcribe.h)
# -----------------------------------------------------------------------------
#
# The TRANSCRIBE_VERSION_MAJOR/MINOR/PATCH macros in the public header are the
# one place the library version is defined. Parse them here, before project(),
# so the CMake project version, the shared-library VERSION/SOVERSION, and the
# value transcribe_version() reports all derive from the same source and cannot
# drift. To bump the library version, edit include/transcribe.h.
file(READ "${CMAKE_CURRENT_SOURCE_DIR}/include/transcribe.h" _transcribe_header)
string(REGEX MATCH "define +TRANSCRIBE_VERSION_MAJOR +([0-9]+)" _ "${_transcribe_header}")
set(TRANSCRIBE_VERSION_MAJOR "${CMAKE_MATCH_1}")
string(REGEX MATCH "define +TRANSCRIBE_VERSION_MINOR +([0-9]+)" _ "${_transcribe_header}")
set(TRANSCRIBE_VERSION_MINOR "${CMAKE_MATCH_1}")
string(REGEX MATCH "define +TRANSCRIBE_VERSION_PATCH +([0-9]+)" _ "${_transcribe_header}")
set(TRANSCRIBE_VERSION_PATCH "${CMAKE_MATCH_1}")
if(NOT TRANSCRIBE_VERSION_MAJOR MATCHES "^[0-9]+$" OR
   NOT TRANSCRIBE_VERSION_MINOR MATCHES "^[0-9]+$" OR
   NOT TRANSCRIBE_VERSION_PATCH MATCHES "^[0-9]+$")
    message(FATAL_ERROR
        "Failed to parse TRANSCRIBE_VERSION_{MAJOR,MINOR,PATCH} from "
        "include/transcribe.h")
endif()

project(transcribe
    VERSION ${TRANSCRIBE_VERSION_MAJOR}.${TRANSCRIBE_VERSION_MINOR}.${TRANSCRIBE_VERSION_PATCH}
    LANGUAGES C CXX)

# Short git commit for transcribe_version_commit(), captured at configure time.
# Best-effort: source tarballs and non-git trees fall back to "unknown". This is
# a configure-time snapshot (same posture as ggml's GGML_COMMIT); reconfigure to
# refresh it.
set(TRANSCRIBE_BUILD_COMMIT "unknown")
find_package(Git QUIET)
if(GIT_FOUND AND EXISTS "${CMAKE_CURRENT_SOURCE_DIR}/.git")
    execute_process(
        COMMAND ${GIT_EXECUTABLE} -C "${CMAKE_CURRENT_SOURCE_DIR}" rev-parse --short HEAD
        OUTPUT_VARIABLE _transcribe_git_sha
        OUTPUT_STRIP_TRAILING_WHITESPACE
        ERROR_QUIET)
    if(_transcribe_git_sha)
        set(TRANSCRIBE_BUILD_COMMIT "${_transcribe_git_sha}")
    endif()
endif()
message(STATUS "transcribe version: ${PROJECT_VERSION} (commit ${TRANSCRIBE_BUILD_COMMIT})")

# -----------------------------------------------------------------------------
# Standards
# -----------------------------------------------------------------------------

set(CMAKE_C_STANDARD            11)
set(CMAKE_C_STANDARD_REQUIRED   ON)
set(CMAKE_CXX_STANDARD          17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS        OFF)

if(NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)
    set(CMAKE_BUILD_TYPE Release CACHE STRING "" FORCE)
endif()

include(GNUInstallDirs)

# Hidden symbol visibility by default per PLAN.md.
set(CMAKE_C_VISIBILITY_PRESET       hidden)
set(CMAKE_CXX_VISIBILITY_PRESET     hidden)
set(CMAKE_VISIBILITY_INLINES_HIDDEN ON)

# -----------------------------------------------------------------------------
# Python wheel build (scikit-build-core sets SKBUILD)
# -----------------------------------------------------------------------------
#
# The repo-root pyproject.toml builds the transcribe-cpp-native provider
# package through scikit-build-core. Force the library shape that package
# ships (shared libtranscribe, no tests/examples/tools) and default to the
# official wheel-hygiene posture: no OpenMP and no non-Apple system BLAS, so
# nothing is vendored that collides with PyTorch/NumPy/MKL in-process, and
# Metal shaders embedded so the artifact has no sidecar files. The hygiene
# defaults (not the shape) can be overridden with -D for local source builds
# that want them. CMP0077 is NEW here, so option() below respects these.
if(SKBUILD)
    set(TRANSCRIBE_BUILD_SHARED   ON)
    set(TRANSCRIBE_BUILD_TESTS    OFF)
    set(TRANSCRIBE_BUILD_EXAMPLES OFF)
    set(TRANSCRIBE_BUILD_TOOLS    OFF)
    if(NOT DEFINED CACHE{TRANSCRIBE_USE_OPENMP})
        set(TRANSCRIBE_USE_OPENMP OFF)
    endif()
    if(NOT DEFINED CACHE{TRANSCRIBE_USE_SYSTEM_BLAS})
        set(TRANSCRIBE_USE_SYSTEM_BLAS OFF)
    endif()
    if(NOT DEFINED CACHE{GGML_METAL_EMBED_LIBRARY})
        set(GGML_METAL_EMBED_LIBRARY ON CACHE BOOL "" FORCE)
    endif()
endif()

# -----------------------------------------------------------------------------
# Apple Silicon detection (drives Metal default)
# -----------------------------------------------------------------------------

set(TRANSCRIBE_IS_APPLE_SILICON OFF)
if(APPLE AND CMAKE_SYSTEM_PROCESSOR MATCHES "arm64|aarch64")
    set(TRANSCRIBE_IS_APPLE_SILICON ON)
endif()

# -----------------------------------------------------------------------------
# Options
# -----------------------------------------------------------------------------

option(TRANSCRIBE_BUILD_TESTS    "Build unit / smoke tests" ON)
option(TRANSCRIBE_BUILD_EXAMPLES "Build example CLI"        ON)
option(TRANSCRIBE_BUILD_TOOLS    "Build inspect / quantize" OFF)  # post pass 2
# Numerical-parity validation hooks: reference-mel injection (TRANSCRIBE_MEL_FROM_REF)
# and per-layer tensor dumps (TRANSCRIBE_DUMP_ALL_BLOCKS / _SUB_BLOCKS). OFF for
# release so the hook code is not compiled into shipped binaries — the env-var
# reads, ref-mel loaders, and dump scaffolding all live behind this guard, so
# their names do not even appear in a release binary's strings. The validation
# pipeline (scripts/validate.py, the Modal sweep image) builds with this ON;
# validate.py hard-fails --mel-from-ref against an OFF build rather than
# silently ignoring it.
option(TRANSCRIBE_ENABLE_VALIDATION_HOOKS
                                 "Compile in numerical-parity validation hooks" OFF)
option(TRANSCRIBE_METAL          "Enable Metal backend"     ${TRANSCRIBE_IS_APPLE_SILICON})
option(TRANSCRIBE_VULKAN         "Enable Vulkan backend"    OFF)  # post-v1
option(TRANSCRIBE_CUDA           "Enable CUDA backend"      OFF)  # opt-in; Linux + nvcc
option(TRANSCRIBE_SANITIZE       "Enable ASan + UBSan"      OFF)
option(TRANSCRIBE_LTO            "Enable LTO in Release"    OFF)

# Library link mode and wheel-hygiene switches.
#
# TRANSCRIBE_BUILD_SHARED OFF (default) builds a static libtranscribe + static
# ggml, so `cmake -B build` yields a self-contained transcribe-cli with no
# libtranscribe/libggml to chase at runtime — the posture every porting, test,
# and bench workflow relies on. The Python wheel/provider build sets it ON to
# produce a shared libtranscribe the FFI layer can dlopen.
#
# TRANSCRIBE_USE_OPENMP defaults OFF: we use ggml's native CPU threadpool
# everywhere (see the OpenMP CENTRAL POLICY below — OpenMP's process-global pool
# is a teardown/coexistence liability for an embeddable library, and the native
# pool is now correct on MSVC and under oversubscription). Opt in with
# -DTRANSCRIBE_USE_OPENMP=ON. TRANSCRIBE_USE_SYSTEM_BLAS stays ON for the
# Accelerate/system-BLAS host decoder; official provider wheels pass it OFF so no
# OpenBLAS/MKL runtime is vendored where it can collide with PyTorch/NumPy.
option(TRANSCRIBE_BUILD_SHARED    "Build libtranscribe + ggml as shared libraries"  OFF)
option(TRANSCRIBE_USE_OPENMP      "Use OpenMP for ggml's CPU threadpool"             OFF)
option(TRANSCRIBE_USE_SYSTEM_BLAS "Link non-Apple system BLAS for the host decoder"  ON)

# Dynamic ggml backends: each backend (CPU, Vulkan, CUDA, ...) becomes a
# separate loadable module living NEXT TO libtranscribe instead of being
# compiled in. This is the provider-directory artifact shape the Python
# wheels ship on Linux/Windows: the Vulkan module is present by default and
# simply fails to load (skipped; CPU keeps working) on machines without a
# Vulkan loader/driver. The host loads the module directory once via
# transcribe_init_backends(). Requires TRANSCRIBE_BUILD_SHARED=ON.
option(TRANSCRIBE_GGML_BACKEND_DL "Build ggml backends as loadable modules" OFF)

# Conservative x86 floor: default GGML_NATIVE plus EVERY x86 SIMD tier to
# OFF so the one binary runs on baseline x86-64 without SIGILL. This is the
# single switch behind every official-wheel CPU posture (the per-flag lists
# used to be mirrored across the root pyproject lanes, the cu12 pyproject,
# and CMakePresets.json — one option, one place to drift).
#
# It is a DEFAULT, not a clamp (same posture as the SKBUILD hygiene knobs
# above): an explicit -DGGML_AVX2=ON etc. on the command line still wins, so
# a user can take the conservative floor and selectively re-enable tiers for
# their machine. With the option OFF (the default) the raw GGML_* flags are
# untouched and fully tunable, exactly as without this option.
#
# Interaction with GGML_CPU_ALL_VARIANTS=ON (the fat-CPU wheel lanes): ggml
# gates the per-flag options behind `if(NOT GGML_CPU_ALL_VARIANTS)`, so
# these are inert there and serve as the explicit floor if ALL_VARIANTS is
# ever disabled. Inert (harmless) on non-x86 targets: ggml's Linux-ARM tier
# list has its own flags.
option(TRANSCRIBE_X86_CONSERVATIVE
    "Default GGML_NATIVE and every x86 SIMD tier to OFF (SIGILL-safe baseline; explicit -DGGML_* still wins)" OFF)

# Install rules for non-Python consumers: headers + the ABI digest +
# libtranscribe + the transcribe-link.json link manifest (ggml's own install
# rules supply the ggml libs/headers). This is what the Rust -sys crate's
# build.rs consumes (`cmake --install` into a staging prefix, then link per
# the manifest) — see cmake/transcribe-install.cmake. Default ON for
# top-level builds; OFF as a subproject, and never active under SKBUILD
# (the wheel ships its own component-filtered install rules instead).
if(CMAKE_SOURCE_DIR STREQUAL CMAKE_CURRENT_SOURCE_DIR AND NOT SKBUILD)
    set(_transcribe_install_default ON)
else()
    set(_transcribe_install_default OFF)
endif()
option(TRANSCRIBE_INSTALL
    "Install headers, libtranscribe, and the link manifest" ${_transcribe_install_default})

# Real-model gated tests. OFF by default because the tests need a
# converted Parakeet GGUF on disk (~2.4 GB) and CI can't ship one.
# When ON, the test reads TRANSCRIBE_PARAKEET_GGUF from the
# environment at run time. See tests/parakeet_real_smoke.cpp.
option(TRANSCRIBE_BUILD_REAL_MODEL_TESTS
    "Build tests that load real (multi-GB) model files via env var" OFF)

# -----------------------------------------------------------------------------
# Warnings (no -Werror in v1 per PLAN.md)
# -----------------------------------------------------------------------------
#
# transcribe_apply_warnings(target): apply our project warning flags to one
# target. Used per-target so we don't pollute the vendored ggml subdirectory
# with -Wshadow / -Wpedantic, which fire on a lot of legitimate ggml code.

function(transcribe_apply_warnings tgt)
    if(CMAKE_CXX_COMPILER_ID MATCHES "Clang|GNU")
        target_compile_options(${tgt} PRIVATE
            -Wall
            -Wextra
            -Wshadow
            -Wpedantic
            -Wno-unused-parameter
            # The output-side TRANSCRIBE_*_INIT macros (timings,
            # stream_update, capabilities, whisper_chunk_trace, ...)
            # are deliberate zero-fill aggregate initializers:
            # struct_size is set, every other field falls to
            # initializer zero by C aggregate-init rules. The
            # zero-means-absent contract is documented and load-bearing.
            # -Wmissing-field-initializers would fire on every use of
            # those macros; the warning's signal-to-noise on this
            # codebase is too low to justify keeping it on.
            -Wno-missing-field-initializers
        )
    endif()
endfunction()

# Sanitizers must be applied to every target in the build (including ggml)
# so the resulting binary is consistent. add_compile_options /
# add_link_options is the right scope for this.
#
# -fno-sanitize-recover=undefined makes UBSan reports fatal at runtime
# instead of merely printing them. ASan already aborts on error by
# default; UBSan does not, which would otherwise let ctest report
# success while the sanitizer printed real diagnostics.
if(TRANSCRIBE_SANITIZE)
    add_compile_options(
        -fsanitize=address,undefined
        -fno-sanitize-recover=undefined
        # ggml's `incr_ptr_aligned` (ggml/src/ggml.c) uses the
        # standard "use NULL as base, walk by offsets to compute
        # struct layout sizes" trick. UBSan flags this as a
        # `pointer-overflow` runtime error even though it's a
        # widely-used and safe-in-practice C idiom. Make this
        # specific check non-fatal so the encoder run path doesn't
        # abort under build-san; real bugs in our own code still
        # fail loud (the rest of -fno-sanitize-recover=undefined
        # is unaffected). When ggml gets patched upstream this can
        # come back out.
        -fsanitize-recover=pointer-overflow
        -fno-omit-frame-pointer
    )
    add_link_options(-fsanitize=address,undefined)
endif()

# -----------------------------------------------------------------------------
# Output layout
# -----------------------------------------------------------------------------
#
# Put all binaries in build/bin/ regardless of which subdirectory built them.
# ggml's CMakeLists already does this when invoked standalone; pinning it
# at the top level keeps the same layout when ggml is added as a subdirectory.

set(CMAKE_RUNTIME_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/bin)

# -----------------------------------------------------------------------------
# Vendored ggml
# -----------------------------------------------------------------------------
#
# ggml lives at the repo root (matching whisper.cpp / llama.cpp). The pinned
# upstream SHA is recorded in ggml/UPSTREAM. We map our TRANSCRIBE_* options
# onto ggml's GGML_* options BEFORE add_subdirectory() so ggml's option()
# calls become no-ops and our defaults win.

set(GGML_METAL    ${TRANSCRIBE_METAL}  CACHE BOOL "" FORCE)
set(GGML_VULKAN   ${TRANSCRIBE_VULKAN} CACHE BOOL "" FORCE)
set(GGML_CUDA     ${TRANSCRIBE_CUDA}   CACHE BOOL "" FORCE)
set(GGML_BLAS OFF CACHE BOOL "" FORCE)
# tinyBLAS (Justine Tunney's llamafile_sgemm CPU kernels): ~29% faster encoder on
# CPU (q8_0 GEMM), numerically WER-equivalent. On by default; CPU-backend only.
set(GGML_LLAMAFILE ON CACHE BOOL "" FORCE)

# Conservative x86 floor (see the option's comment above): one switch fans
# out to GGML_NATIVE plus the full x86 SIMD tier list. Placed BEFORE
# add_subdirectory(ggml) so ggml's own option() defaults never win — but
# NOT forced: a flag the user already put in the cache (-DGGML_AVX2=ON) is
# left alone, so the floor stays selectively tunable per machine.
if(TRANSCRIBE_X86_CONSERVATIVE)
    foreach(_simd_flag
            GGML_NATIVE
            GGML_SSE42 GGML_AVX GGML_AVX_VNNI GGML_AVX2 GGML_BMI2
            GGML_FMA GGML_F16C
            GGML_AVX512 GGML_AVX512_VBMI GGML_AVX512_VNNI GGML_AVX512_BF16)
        if(NOT DEFINED CACHE{${_simd_flag}})
            set(${_simd_flag} OFF CACHE BOOL "")
        endif()
    endforeach()
endif()

# OpenMP — CENTRAL POLICY.
#
# DEFAULT: OFF. We use ggml's native CPU threadpool everywhere; OpenMP is an
# opt-in (-DTRANSCRIBE_USE_OPENMP=ON), not the default. The TRANSCRIBE_USE_OPENMP
# knob simply drives ggml's GGML_OPENMP.
#
# Why native, not OpenMP — OpenMP's runtime pool is process-global and outlives
# any single compute, which is a liability for an embeddable/dlopen'd library:
#   - Teardown crash: a binding (node/koffi, ctypes, ...) calls in on a worker
#     thread, libgomp/vcomp spawns its pool there, and at process teardown the
#     loader unmaps the runtime out from under those still-live pool threads ->
#     SIGSEGV / 0xC0000005. The native pool joins its threads per graph_compute,
#     so nothing outlives the call and there is nothing to unmap-race.
#   - Coexistence: a vendored libgomp/libiomp can collide with numpy/torch/MKL's
#     own OpenMP in one process. The native pool vendors no second runtime.
#
# This used to be a per-consumer SPLIT because ggml's native barrier deadlocked
# under MSVC (its spin `relax` was a no-op on MSVC, so a waiter starved an
# un-arrived worker under oversubscription) — OpenMP was the only working
# multi-threaded CPU path on Windows. That barrier is now fixed (ggml-cpu.c:
# YieldProcessor() relax + a bounded-spin ggml_thread_yield fallback), so the
# native pool is correct on MSVC and under oversubscription. The split is gone:
#   - Rust binding: no longer needs -DGGML_OPENMP=ON on Windows; native pool.
#   - Python wheels: already vendored no OpenMP; multi-threaded CPU now works on
#     every platform (the old "CPU-on-Windows unsupported" limitation is lifted).
#   - perf: native persistent/ephemeral pool is on par with OpenMP (it is
#     llama.cpp's default). Thread count is sized affinity-aware in
#     transcribe-batch-util (default_n_threads) to avoid needless oversubscription.
#
# So: honor an explicit -DGGML_OPENMP=... (opt-in); otherwise force OFF.
if(NOT TRANSCRIBE_USE_OPENMP AND NOT DEFINED CACHE{GGML_OPENMP})
    set(GGML_OPENMP OFF CACHE BOOL "" FORCE)
endif()

# We don't want ggml's tests / examples leaking into our build tree. They
# default to OFF when ggml is added as a subdirectory (GGML_STANDALONE OFF),
# but pin them anyway so the contract is explicit.
set(GGML_BUILD_TESTS    OFF CACHE BOOL "" FORCE)
set(GGML_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)

# Library link mode. Default OFF (static) keeps the self-contained CLI/test/
# bench build with no libtranscribe/libggml to chase at runtime; the Python
# wheel/provider build sets TRANSCRIBE_BUILD_SHARED=ON for a shared
# libtranscribe (+ libggml) the FFI layer can dlopen. Both ggml and
# libtranscribe follow this single switch. Symbol visibility is already hidden
# by default (set above), so a shared lib exports only the TRANSCRIBE_API
# surface.
set(BUILD_SHARED_LIBS ${TRANSCRIBE_BUILD_SHARED} CACHE BOOL "" FORCE)

# Dynamic backend modules (see the option's comment above). ggml requires
# shared libs for GGML_BACKEND_DL, so gate it on TRANSCRIBE_BUILD_SHARED
# here for a clear configure-time error.
if(TRANSCRIBE_GGML_BACKEND_DL)
    if(NOT TRANSCRIBE_BUILD_SHARED)
        message(FATAL_ERROR
            "TRANSCRIBE_GGML_BACKEND_DL requires TRANSCRIBE_BUILD_SHARED=ON "
            "(backend modules are shared libraries loaded at runtime)")
    endif()
    set(GGML_BACKEND_DL ON CACHE BOOL "" FORCE)
    # The package-local default loader resolves the directory that contains
    # libtranscribe itself. Install backend modules there too, unless a caller
    # explicitly chose a different provider layout and will pass it to
    # transcribe_init_backends(dir).
    #
    # Test emptiness, not DEFINED: ggml declares GGML_BACKEND_DIR as an empty
    # cache var of its own, so on any reconfigure it is already DEFINED (as "")
    # and a `NOT DEFINED CACHE{...}` guard would skip our override, leaving the
    # modules in ggml's default bin/ where the lib-dir loader cannot find them.
    # `NOT GGML_BACKEND_DIR` treats an empty/unset value as "ours to set" while
    # still respecting a non-empty caller-provided path.
    if(NOT GGML_BACKEND_DIR)
        # The directory that holds libtranscribe itself — which is where the
        # package-local loader scans. That's bin/ on Windows (a DLL is a RUNTIME
        # artifact installed to bin, so transcribe.dll lives there) and lib/
        # elsewhere. Using lib/ unconditionally would strand the modules away
        # from transcribe.dll on Windows, where the loader can't find them.
        if(WIN32)
            set(GGML_BACKEND_DIR "${CMAKE_INSTALL_BINDIR}" CACHE PATH "" FORCE)
        else()
            set(GGML_BACKEND_DIR "${CMAKE_INSTALL_LIBDIR}" CACHE PATH "" FORCE)
        endif()
    endif()
    # ggml hard-errors on GGML_NATIVE + GGML_BACKEND_DL (x86): native tuning
    # is incompatible with feature-scored modules. A module build is a
    # distribution build, so never tune it to the build host.
    set(GGML_NATIVE OFF CACHE BOOL "" FORCE)
endif()

# MSVC: use /EHs so exceptions thrown by ggml's C-ABI frames unwind to
# transcribe.cpp's public-entry guards. Apply before ggml and src are added.
if(MSVC)
    string(REPLACE "/EHsc" "/EHs" CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS}")
    if(NOT CMAKE_CXX_FLAGS MATCHES "/EHs")
        set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} /EHs")
    endif()
endif()

# ggml's CMake declares its own warning flags; let it.
add_subdirectory(ggml)

# -----------------------------------------------------------------------------
# Subdirs
# -----------------------------------------------------------------------------

add_subdirectory(src)

if(TRANSCRIBE_BUILD_EXAMPLES)
    add_subdirectory(examples)
endif()

if(TRANSCRIBE_BUILD_TESTS)
    enable_testing()
    add_subdirectory(tests)
endif()

if(TRANSCRIBE_BUILD_TOOLS)
    add_subdirectory(tools)
endif()

# Python provider-wheel packaging (after subdirs: needs the target list and
# ggml's GGML_AVAILABLE_BACKENDS).
if(SKBUILD)
    include(cmake/python-wheel-install.cmake)
endif()

# Consumer install rules (same placement constraint as above).
if(TRANSCRIBE_INSTALL AND NOT SKBUILD)
    include(cmake/transcribe-install.cmake)
endif()
