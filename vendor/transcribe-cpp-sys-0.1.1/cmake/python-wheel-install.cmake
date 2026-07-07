# Install rules for the transcribe-cpp-native Python provider wheel.
#
# Only included when scikit-build-core drives the build (SKBUILD is set, see
# the gate in the top-level CMakeLists.txt). Everything the wheel ships is
# COMPONENT wheel; pyproject.toml restricts packaging to that component, so
# the vendored ggml's default-component install rules (headers, cmake config,
# pkg-config) never leak into the wheel.
#
# The artifact directory is FLAT: every shared library and ggml backend module
# sits next to libtranscribe in transcribe_cpp_native/_native/, resolved via
# $ORIGIN / @loader_path rpaths (Windows uses os.add_dll_directory in the
# binding's loader). transcribe_init_backends() scans exactly this directory
# for backend modules, so module search never escapes the package.

# --- the library set --------------------------------------------------------
# ggml maintains GGML_AVAILABLE_BACKENDS (internal cache) as backends register;
# in GGML_BACKEND_DL builds these are MODULE libraries (the provider-directory
# shape), otherwise ordinary shared libraries linked into libggml. The
# backend-target/kind classification is shared with transcribe-install.cmake.
include("${CMAKE_CURRENT_LIST_DIR}/transcribe-backend-kinds.cmake")
transcribe_backend_kinds(_backend_kinds _backend_targets)
set(_wheel_targets transcribe ggml ggml-base ${_backend_targets})
list(REMOVE_DUPLICATES _wheel_targets)

foreach(_tgt IN LISTS _wheel_targets)
    # Strip VERSION/SOVERSION decoration inside the wheel: versioned library
    # names install as symlink chains (libggml.so -> libggml.so.0 -> ...), and
    # wheels cannot carry symlinks — zip would materialize each link as a full
    # duplicate copy. Undecorated names also make the SONAME/install-name the
    # plain file name, so sibling resolution needs nothing but the rpath.
    # Pre-1.0 the binding's exact version/header-hash check is the real
    # compatibility gate, not the SONAME.
    set_property(TARGET ${_tgt} PROPERTY VERSION)
    set_property(TARGET ${_tgt} PROPERTY SOVERSION)
    if(APPLE)
        set_property(TARGET ${_tgt} PROPERTY INSTALL_RPATH "@loader_path")
    else()
        set_property(TARGET ${_tgt} PROPERTY INSTALL_RPATH "$ORIGIN")
    endif()
    install(TARGETS ${_tgt}
        # Shared libraries / backend modules (RUNTIME = DLLs on Windows).
        LIBRARY DESTINATION _native COMPONENT wheel
        RUNTIME DESTINATION _native COMPONENT wheel
        # Windows import .lib stubs: link-time only, never packaged.
        ARCHIVE DESTINATION _native-dev COMPONENT wheel-dev)
endforeach()

# --- the build-time contract stamp (_contract.py) ---------------------------
# The binding hard-fails on a provider whose version or public-header hash
# disagrees with it (_library.validate_contract). Version comes from the
# header macros via PROJECT_VERSION; the header hash comes from the
# binding-neutral include/transcribe.abihash — emitted by the FFI generator
# (bindings/python/_generate/generate.py, the hash oracle) and drift-gated in
# CI alongside _generated.py, so wheel and binding are stamped from the same
# source without this file depending on a Python binding artifact.
file(STRINGS "${CMAKE_CURRENT_SOURCE_DIR}/include/transcribe.abihash"
     _public_header_hash LIMIT_COUNT 1)
string(STRIP "${_public_header_hash}" _public_header_hash)
if(NOT _public_header_hash MATCHES "^[0-9a-f]+$")
    message(FATAL_ERROR
        "python-wheel-install: failed to read the ABI digest from "
        "include/transcribe.abihash (regenerate with "
        "bindings/python/_generate/generate.py)")
endif()

# Backend kinds this artifact supports, for the descriptor's `backends` field
# (and the binding's accelerated-first provider ranking). Classified by
# transcribe_backend_kinds above: "ggml-" stripped, CPU ISA variants
# (ggml-cpu-haswell, ...) collapsed to "cpu", accelerated kinds first.
list(JOIN _backend_kinds "\", \"" _backends_joined)
set(TRANSCRIBE_WHEEL_BACKENDS_PY "\"${_backends_joined}\"")
set(TRANSCRIBE_PUBLIC_HEADER_HASH "${_public_header_hash}")

configure_file(
    "${CMAKE_CURRENT_SOURCE_DIR}/bindings/python-native/_contract.py.in"
    "${CMAKE_CURRENT_BINARY_DIR}/python-wheel/_contract.py"
    @ONLY)
install(FILES "${CMAKE_CURRENT_BINARY_DIR}/python-wheel/_contract.py"
        DESTINATION . COMPONENT wheel)

# --- the language-neutral contract twin (contract.json) ---------------------
# Same stamping pass as _contract.py, but JSON and INSIDE _native/, so it
# travels with the native bytes when CI extracts the directory into a
# transcribe-native-<tuple> bundle (the canonical artifact non-Python
# ecosystems consume). Any binding validates version + header_hash before
# dlopen — the same contract the Python provider enforces. `lane` records
# which official wheel lane built the artifact (null for sdist/dev builds).
if(DEFINED ENV{TRANSCRIBE_WHEEL_LANE} AND NOT "$ENV{TRANSCRIBE_WHEEL_LANE}" STREQUAL "")
    set(_contract_lane_json "\"$ENV{TRANSCRIBE_WHEEL_LANE}\"")
else()
    set(_contract_lane_json "null")
endif()
file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/python-wheel/contract.json"
"{
  \"version\": \"${PROJECT_VERSION}\",
  \"header_hash\": \"${_public_header_hash}\",
  \"backends\": [\"${_backends_joined}\"],
  \"lane\": ${_contract_lane_json}
}
")
install(FILES "${CMAKE_CURRENT_BINARY_DIR}/python-wheel/contract.json"
        DESTINATION _native COMPONENT wheel)

message(STATUS
    "python wheel: transcribe-cpp-native ${PROJECT_VERSION} "
    "(header ${_public_header_hash}, backends: ${_backend_kinds})")
