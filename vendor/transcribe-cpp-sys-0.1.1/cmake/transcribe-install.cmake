# Install rules for consuming the C library OUTSIDE Python packaging —
# the foundation the Rust -sys crate's build.rs (and any plain C/C++
# consumer) builds on. Included from the top-level CMakeLists.txt when
# TRANSCRIBE_INSTALL is ON (default for top-level non-SKBUILD builds; the
# Python wheel has its own component-filtered install in
# python-wheel-install.cmake).
#
# What `cmake --install` produces:
#   include/   transcribe.h, transcribe.abihash, transcribe/*.h
#              (plus ggml's public headers via ggml's own install rules)
#   lib/       libtranscribe (static or shared per TRANSCRIBE_BUILD_SHARED)
#              + ggml/ggml-base/backend libs (ggml's own install rules)
#              + transcribe-link.json (the link manifest, see below)
#
# transcribe-link.json is the machine-readable link interface for non-CMake
# consumers: which archives to link in which order, plus the system
# libraries/frameworks/flags they drag in. It is generated from the
# configured build (backend set, OpenMP/BLAS posture), not hardcoded by the
# consumer — the whisper-rs drift class this avoids. The link-smoke CI lane
# (scripts/ci/link_smoke.py) compiles a toy C consumer from NOTHING but this
# manifest, in both postures, so a wrong manifest is a red check, not a
# downstream surprise.
#
# Windows note: the static-posture system-library translation below is
# provisional (MSVC has no -framework/-lstdc++); it gets exercised and completed
# when the Rust branch does its MSVC shakeout. The smoke lanes cover linux +
# macos. (miniz is vendored into libtranscribe, so there is no external
# compression library to translate here anymore.)

include(GNUInstallDirs)
include("${CMAKE_CURRENT_LIST_DIR}/transcribe-backend-kinds.cmake")

# --- headers + the ABI digest ------------------------------------------------
install(FILES
    "${CMAKE_CURRENT_SOURCE_DIR}/include/transcribe.h"
    "${CMAKE_CURRENT_SOURCE_DIR}/include/transcribe.abihash"
    DESTINATION ${CMAKE_INSTALL_INCLUDEDIR})
install(DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}/include/transcribe"
    DESTINATION ${CMAKE_INSTALL_INCLUDEDIR}
    FILES_MATCHING PATTERN "*.h")

# --- libtranscribe ------------------------------------------------------------
# ggml's own install rules cover ggml / ggml-base. They ALSO declare an install
# for each backend module, but ggml_add_backend_library() uses `LIBRARY
# DESTINATION` only — and a MODULE library's .dll is the RUNTIME artifact on
# Windows, so that rule installs nothing there: the loadable ggml-*.dll backend
# modules never reach bin/, leaving a shared/dynamic-backends install with zero
# compute backends next to libtranscribe. We re-install their RUNTIME artifact
# below (mirrors what python-wheel-install.cmake already does for the wheel).
# In shared mode, give every library a self-referential rpath so the
# installed lib dir is self-contained: under RUNPATH semantics the LOADING
# object's rpath resolves its deps, so libtranscribe needs $ORIGIN to find
# its sibling libggml*, not just the consumer binary.
transcribe_backend_kinds(_kinds _backend_targets)
if(TRANSCRIBE_BUILD_SHARED)
    foreach(_tgt transcribe ggml ggml-base ${_backend_targets})
        if(APPLE)
            set_property(TARGET ${_tgt} PROPERTY INSTALL_RPATH "@loader_path")
        else()
            set_property(TARGET ${_tgt} PROPERTY INSTALL_RPATH "$ORIGIN")
        endif()
    endforeach()
endif()
install(TARGETS transcribe
    LIBRARY DESTINATION ${CMAKE_INSTALL_LIBDIR}
    ARCHIVE DESTINATION ${CMAKE_INSTALL_LIBDIR}
    RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR})

# Install the ggml backend MODULE libraries beside libtranscribe on Windows.
# In CMake's install() model a MODULE library is a LIBRARY artifact on EVERY
# platform — Windows included; its .dll is NOT a RUNTIME artifact — so the
# destination must be LIBRARY. A RUNTIME-only destination hard-errors at configure
# with "install TARGETS given no LIBRARY DESTINATION for module target ...", which
# broke every Windows dynamic-backends configure. With LIBRARY DESTINATION the
# loadable ggml-cpu-*/ggml-vulkan DLLs land in GGML_BACKEND_DIR — the directory
# holding libtranscribe (bin on Windows; set in the top-level CMakeLists) — so
# they sit next to transcribe.dll where the package-local loader scans.
# `_backend_targets` (from transcribe_backend_kinds above) is the full module set,
# incl. every CPU ISA variant. RUNTIME is kept too as a harmless belt-and-braces.
# No-op on Unix, where ggml's own LIBRARY rule already installs the .so.
if(WIN32 AND TRANSCRIBE_GGML_BACKEND_DL AND _backend_targets)
    install(TARGETS ${_backend_targets}
            LIBRARY DESTINATION ${GGML_BACKEND_DIR}
            RUNTIME DESTINATION ${GGML_BACKEND_DIR})
endif()

# --- the link manifest (transcribe-link.json) --------------------------------
set(_libraries transcribe)
set(_library_paths "")
set(_system_libs "")
set(_frameworks "")
set(_link_flags "")

if(NOT TRANSCRIBE_BUILD_SHARED)
    # Static: the consumer links the whole archive set. Order is
    # single-pass-ld safe: each archive's undefined refs resolve in a later
    # one (transcribe -> ggml -> backends -> ggml-base).
    list(APPEND _libraries ggml ${_backend_targets} ggml-base)

    # The archives are C++; the consumer may be C or Rust.
    if(APPLE)
        list(APPEND _system_libs c++ m)
    elseif(WIN32)
        # MSVC links the CRT and the C++ runtime implicitly. ggml-cpu reads the
        # registry for CPU feature detection (RegOpenKeyEx/RegCloseKey), so the
        # static consumer must pull in advapi32 — ggml's own CMake never links
        # it (it rides MSVC default-lib auto-linking the manifest reconstruction
        # loses).
        list(APPEND _system_libs advapi32)
    elseif(UNIX)
        list(APPEND _system_libs stdc++ m pthread dl)
    endif()

    # Translate libtranscribe's PRIVATE link list (the configure-time truth
    # for OpenMP / Accelerate / system BLAS) into consumer terms. The deflate
    # compressor (miniz) is vendored straight into libtranscribe, so it never
    # appears here.
    get_target_property(_transcribe_links transcribe LINK_LIBRARIES)
    foreach(_dep IN LISTS _transcribe_links)
        if(_dep STREQUAL "OpenMP::OpenMP_CXX")
            list(APPEND _link_flags -fopenmp)
        elseif(_dep MATCHES "^-framework (.+)$")
            list(APPEND _frameworks "${CMAKE_MATCH_1}")
        elseif(_dep MATCHES "\\.(a|so[.0-9]*|dylib|tbd|lib)$")
            # Absolute paths (e.g. find_package(BLAS) results).
            list(APPEND _library_paths "${_dep}")
        endif()
    endforeach()

    # Per-backend system dependencies the ggml archives drag in.
    if("metal" IN_LIST _kinds)
        list(APPEND _frameworks Foundation Metal MetalKit)
    endif()
    if("blas" IN_LIST _kinds AND APPLE)
        list(APPEND _frameworks Accelerate)
    endif()
    if("vulkan" IN_LIST _kinds)
        list(APPEND _system_libs vulkan)
    endif()
    if(_frameworks)
        list(REMOVE_DUPLICATES _frameworks)
    endif()
    if(_system_libs)
        list(REMOVE_DUPLICATES _system_libs)
    endif()
endif()
# Shared: the consumer links libtranscribe alone; the ggml libraries are
# runtime dependencies resolved through the rpaths set above.

set(_metal_embed false)
if("metal" IN_LIST _kinds AND GGML_METAL_EMBED_LIBRARY)
    set(_metal_embed true)
endif()
if(TRANSCRIBE_BUILD_SHARED)
    set(_shared_json true)
else()
    set(_shared_json false)
endif()
if(TRANSCRIBE_GGML_BACKEND_DL)
    set(_backend_dl_json true)
    # ggml installs backend MODULES to GGML_BACKEND_DIR when set, else to
    # bin/. Record where, so a consumer knows what directory to hand to
    # transcribe_init_backends() (DL builds compile in NO backends — without
    # that call the process has zero devices).
    if(GGML_BACKEND_DIR)
        set(_module_dir_json "\"${GGML_BACKEND_DIR}\"")
    else()
        set(_module_dir_json "\"${CMAKE_INSTALL_BINDIR}\"")
    endif()
else()
    set(_backend_dl_json false)
    set(_module_dir_json null)
endif()

# JSON list helper: emits `"a", "b"` (empty list -> empty string -> []).
function(_transcribe_json_strings out)
    set(_quoted "")
    foreach(_item IN LISTS ARGN)
        list(APPEND _quoted "\"${_item}\"")
    endforeach()
    list(JOIN _quoted ", " _joined)
    set(${out} "${_joined}" PARENT_SCOPE)
endfunction()
_transcribe_json_strings(_kinds_json ${_kinds})
_transcribe_json_strings(_libraries_json ${_libraries})
_transcribe_json_strings(_library_paths_json ${_library_paths})
_transcribe_json_strings(_system_libs_json ${_system_libs})
_transcribe_json_strings(_frameworks_json ${_frameworks})
_transcribe_json_strings(_link_flags_json ${_link_flags})

file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/transcribe-link.json"
"{
  \"version\": \"${PROJECT_VERSION}\",
  \"commit\": \"${TRANSCRIBE_BUILD_COMMIT}\",
  \"shared\": ${_shared_json},
  \"backend_dl\": ${_backend_dl_json},
  \"module_dir\": ${_module_dir_json},
  \"backends\": [${_kinds_json}],
  \"metal_embed\": ${_metal_embed},
  \"include_dir\": \"${CMAKE_INSTALL_INCLUDEDIR}\",
  \"lib_dir\": \"${CMAKE_INSTALL_LIBDIR}\",
  \"libraries\": [${_libraries_json}],
  \"library_paths\": [${_library_paths_json}],
  \"system_libs\": [${_system_libs_json}],
  \"frameworks\": [${_frameworks_json}],
  \"link_flags\": [${_link_flags_json}]
}
")
install(FILES "${CMAKE_CURRENT_BINARY_DIR}/transcribe-link.json"
    DESTINATION ${CMAKE_INSTALL_LIBDIR})

message(STATUS
    "transcribe install: ${PROJECT_VERSION} shared=${TRANSCRIBE_BUILD_SHARED} "
    "backends: ${_kinds} (link manifest: lib/transcribe-link.json)")
