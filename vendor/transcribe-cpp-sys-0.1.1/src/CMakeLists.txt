# transcribe library.
#
# Sources are listed explicitly. Per-family code lives under arch/<fam>/;
# shared scaffolding (loader, mel frontend, tokenizer, load-common,
# backend classification, flash-policy, ...) lives alongside the
# dispatcher at the top level of src/.
#
# Backend libraries (Metal, Vulkan, ...) are compiled into ggml by the
# top-level CMake via GGML_METAL / GGML_VULKAN. This target discovers
# them at runtime via the ggml device registry; no per-backend compile
# defs are needed on libtranscribe itself.

add_library(transcribe
    transcribe.cpp
    transcribe-loader.cpp
    transcribe-arch.cpp
    transcribe-meta.cpp
    transcribe-kaldi-fbank.cpp
    transcribe-mel.cpp
    transcribe-model.cpp
    transcribe-tokenizer.cpp
    transcribe-unicode.cpp
    transcribe-unicode-data.cpp
    transcribe-debug.cpp
    transcribe-env.cpp
    transcribe-flash-policy.cpp
    transcribe-backend.cpp
    transcribe-load-common.cpp
    transcribe-weights-util.cpp
    transcribe-bin-loader.cpp
    transcribe-batch-util.cpp
    conformer/conformer.cpp
    sanm/sanm.cpp
    causal_lm/causal_lm.cpp
    granite_conformer/shaw_attn.cpp
    arch/parakeet/model.cpp
    arch/parakeet/capabilities.cpp
    arch/parakeet/weights.cpp
    arch/parakeet/encoder.cpp
    arch/parakeet/decoder.cpp
    arch/cohere/model.cpp
    arch/cohere/capabilities.cpp
    arch/cohere/weights.cpp
    arch/cohere/encoder.cpp
    arch/cohere/decoder.cpp
    arch/canary/model.cpp
    arch/canary/capabilities.cpp
    arch/canary/weights.cpp
    arch/canary/encoder.cpp
    arch/canary/decoder.cpp
    arch/qwen3_asr/model.cpp
    arch/qwen3_asr/capabilities.cpp
    arch/qwen3_asr/weights.cpp
    arch/qwen3_asr/encoder.cpp
    arch/qwen3_asr/decoder.cpp
    arch/voxtral/model.cpp
    arch/voxtral/capabilities.cpp
    arch/voxtral/weights.cpp
    arch/voxtral/encoder.cpp
    arch/voxtral/decoder.cpp
    arch/voxtral_realtime/model.cpp
    arch/voxtral_realtime/capabilities.cpp
    arch/voxtral_realtime/weights.cpp
    arch/voxtral_realtime/encoder.cpp
    arch/voxtral_realtime/decoder.cpp
    arch/canary_qwen/model.cpp
    arch/canary_qwen/capabilities.cpp
    arch/canary_qwen/weights.cpp
    arch/canary_qwen/encoder.cpp
    arch/canary_qwen/decoder.cpp
    arch/whisper/model.cpp
    arch/whisper/capabilities.cpp
    arch/whisper/weights.cpp
    arch/whisper/encoder.cpp
    arch/whisper/decoder.cpp
    arch/whisper/bin_load.cpp
    arch/whisper/public.cpp
    arch/moonshine/model.cpp
    arch/moonshine/capabilities.cpp
    arch/moonshine/weights.cpp
    arch/moonshine/encoder.cpp
    arch/moonshine/decoder.cpp
    arch/moonshine_streaming/model.cpp
    arch/moonshine_streaming/capabilities.cpp
    arch/moonshine_streaming/weights.cpp
    arch/moonshine_streaming/encoder.cpp
    arch/moonshine_streaming/decoder.cpp
    arch/sensevoice/model.cpp
    arch/sensevoice/capabilities.cpp
    arch/sensevoice/weights.cpp
    arch/sensevoice/encoder.cpp
    arch/funasr_nano/model.cpp
    arch/funasr_nano/capabilities.cpp
    arch/funasr_nano/weights.cpp
    arch/funasr_nano/encoder.cpp
    arch/funasr_nano/decoder.cpp
    arch/funasr_nano/adaptor.cpp
    arch/gigaam/model.cpp
    arch/gigaam/capabilities.cpp
    arch/gigaam/weights.cpp
    arch/gigaam/encoder.cpp
    arch/gigaam/decoder.cpp
    arch/gigaam/mel.cpp
    arch/granite/model.cpp
    arch/granite/capabilities.cpp
    arch/granite/weights.cpp
    arch/granite/encoder.cpp
    arch/granite/projector.cpp
    arch/granite/decoder.cpp
    arch/granite_nar/model.cpp
    arch/granite_nar/capabilities.cpp
    arch/granite_nar/weights.cpp
    arch/granite_nar/encoder.cpp
    arch/granite_nar/projector.cpp
    arch/granite_nar/decoder.cpp
    arch/medasr/model.cpp
    arch/medasr/capabilities.cpp
    arch/medasr/weights.cpp
    arch/medasr/encoder.cpp
    # Vendored miniz (src/third_party/miniz, see its UPSTREAM file): the
    # deflate compressor behind Whisper's compression-ratio heuristic. Compiled
    # into libtranscribe itself so the static/shared artifact is self-contained
    # (no external zlib to find, link, or stage). Trim flags + warning
    # suppression are set below.
    third_party/miniz/miniz.c
)

target_include_directories(transcribe
    PUBLIC
        $<BUILD_INTERFACE:${CMAKE_SOURCE_DIR}/include>
    PRIVATE
        # Internal headers (transcribe-loader.h, transcribe-arch.h) live
        # alongside the .cpp files. PRIVATE so they never escape into the
        # public include path.
        ${CMAKE_CURRENT_SOURCE_DIR}
)

target_compile_definitions(transcribe
    PRIVATE
        TRANSCRIBE_BUILD
        # Git commit stamped at configure time; surfaced by
        # transcribe_version_commit(). Falls back to "unknown" in the source
        # (see transcribe.cpp) when the build tree has no git metadata.
        TRANSCRIBE_COMMIT="${TRANSCRIBE_BUILD_COMMIT}"
)

if(TRANSCRIBE_GGML_BACKEND_DL)
    target_compile_definitions(transcribe PRIVATE TRANSCRIBE_GGML_BACKEND_DL)
    if(CMAKE_DL_LIBS)
        target_link_libraries(transcribe PRIVATE ${CMAKE_DL_LIBS})
    endif()
endif()

if(TRANSCRIBE_ENABLE_VALIDATION_HOOKS)
    target_compile_definitions(transcribe PRIVATE TRANSCRIBE_ENABLE_VALIDATION_HOOKS)
endif()

# Static posture: tell transcribe.h to drop the Windows __declspec(dllimport/
# export) decoration. PUBLIC so it reaches both this archive's own TUs and any
# CMake consumer linking `transcribe` (C++ tests/examples) — without it a static
# MSVC consumer that includes transcribe.h links against __imp_* thunks the
# archive never provides (LNK2019). Bindgen/Python FFIs carry no __declspec and
# are unaffected. No-op off Windows/MSVC (the header guard is _WIN32-only).
if(NOT TRANSCRIBE_BUILD_SHARED)
    target_compile_definitions(transcribe PUBLIC TRANSCRIBE_STATIC)
endif()

# ggml is the runtime substrate. Linked PRIVATE because the public header
# stays ggml-free (callers never include <ggml.h>).
target_link_libraries(transcribe
    PRIVATE
        ggml
)

# BLAS for the host decoder (cblas_sgemv in decoder.cpp). On Apple we
# link Accelerate directly; elsewhere we probe for OpenBLAS / MKL /
# BLIS via find_package(BLAS). When no BLAS is found the decoder falls
# back to a scalar loop (~14x slower but functionally correct).
# Apple Accelerate is a system framework (always present, nothing to vendor),
# so it stays on unconditionally even for wheels. The non-Apple system-BLAS
# probe is gated on TRANSCRIBE_USE_SYSTEM_BLAS: official provider wheels pass it
# OFF so wheel-repair tools never vendor OpenBLAS/MKL/BLIS where it would fight
# another BLAS/OpenMP runtime in the host process.
if(APPLE)
    target_link_libraries(transcribe PRIVATE "-framework Accelerate")
    target_compile_definitions(transcribe PRIVATE TRANSCRIBE_HAS_BLAS=1)
elseif(TRANSCRIBE_USE_SYSTEM_BLAS)
    # BLA_VENDOR can be set by the user to prefer a specific BLAS
    # (e.g. -DBLA_VENDOR=OpenBLAS). find_package probes the system.
    find_package(BLAS QUIET)
    if(BLAS_FOUND)
        # find_package(BLAS) may find a Fortran-only BLAS (e.g. libblas3
        # on Debian/Ubuntu) that lacks the cblas C wrappers. Verify that
        # cblas.h exists before enabling; otherwise the build compiles
        # cblas_sgemv calls but the linker can't resolve them.
        include(CheckIncludeFile)
        check_include_file(cblas.h HAVE_CBLAS_H)
        if(HAVE_CBLAS_H)
            target_link_libraries(transcribe PRIVATE ${BLAS_LIBRARIES})
            target_compile_definitions(transcribe PRIVATE TRANSCRIBE_HAS_BLAS=1)
            message(STATUS "transcribe: BLAS found — decoder will use cblas_sgemv")
        else()
            message(STATUS "transcribe: BLAS found but cblas.h missing — decoder uses scalar fallback (install libopenblas-dev)")
        endif()
    else()
        message(STATUS "transcribe: no BLAS found — decoder uses scalar fallback")
    endif()
else()
    message(STATUS "transcribe: TRANSCRIBE_USE_SYSTEM_BLAS=OFF — decoder uses scalar fallback (official-wheel posture; no system BLAS vendored)")
endif()

set_target_properties(transcribe PROPERTIES
    OUTPUT_NAME transcribe
    POSITION_INDEPENDENT_CODE ON
    # Versions the shared lib (libtranscribe.0.1.0.dylib + libtranscribe.0.dylib
    # symlink). Ignored for the static archive. SOVERSION is the major only;
    # pre-1.0 the real compatibility backstop is the binding's exact
    # version/header-hash check, not the SONAME.
    VERSION   ${PROJECT_VERSION}
    SOVERSION ${PROJECT_VERSION_MAJOR}
)

# miniz (vendored, src/third_party/miniz) provides the deflate compressor for
# Whisper's temperature-fallback compression_ratio metric (HF
# generation_whisper.py:_retrieve_compression_ratio uses
# len(token_bytes) / len(deflate(token_bytes)) to detect decoder token-id
# loops). Replaces system zlib so there is no find_package(ZLIB) / vcpkg
# dependency and nothing extra to stage into the install link manifest.
#
# Trim flags reach both miniz.c and its only consumer (arch/whisper/model.cpp),
# so they are PRIVATE on the target, not per-source:
#   NO_INFLATE_APIS  compress-only (this also makes miniz.h auto-define
#                    MINIZ_NO_ARCHIVE_APIS, so ZIP is off too — defining the
#                    latter here as well would clash with that and warn).
#   NO_STDIO         no file I/O    NO_TIME      no wall-clock
#   NO_ZLIB_COMPATIBLE_NAMES  don't export global compress/crc32/Z_OK/... that
#                             could collide with a real zlib downstream; call
#                             sites use the mz_-prefixed native API instead.
target_compile_definitions(transcribe PRIVATE
    MINIZ_NO_INFLATE_APIS
    MINIZ_NO_STDIO
    MINIZ_NO_TIME
    MINIZ_NO_ZLIB_COMPATIBLE_NAMES
)

# miniz is third-party C; its style trips our -Wshadow/-Wpedantic/-Wextra set.
# Silence warnings for this one translation unit only (-w / MSVC /w wins over
# the project flags transcribe_apply_warnings appends to the whole target).
if(MSVC)
    set_source_files_properties(third_party/miniz/miniz.c PROPERTIES COMPILE_OPTIONS "/w")
else()
    set_source_files_properties(third_party/miniz/miniz.c PROPERTIES COMPILE_OPTIONS "-w")
endif()

if(WIN32)
    # Windows toolchains hide M_PI and friends behind _USE_MATH_DEFINES
    # (used by the mel frontends and Whisper's bin loader). MSVC and MinGW
    # both honor it — MinGW's strict-C++17 mode hides M_PI the same way.
    target_compile_definitions(transcribe PRIVATE _USE_MATH_DEFINES)
endif()

transcribe_apply_warnings(transcribe)
