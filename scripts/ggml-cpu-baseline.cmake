# CMake toolchain file used during whisper-rs-sys (ggml) build to keep the
# resulting binary runnable on older x86_64 CPUs. Loaded via CMAKE_TOOLCHAIN_FILE
# so the `set(... CACHE ... FORCE)` calls take effect BEFORE ggml's CMakeLists
# defines the corresponding `option(...)`.
#
# Why this exists:
#   Without it, ggml defaults `GGML_NATIVE=ON` and the build host's `-march=native`
#   bakes in whatever ISA the build machine happens to have (often AVX2 or
#   AVX-512). End users on older CPUs (Sandy Bridge, Ivy Bridge, low-end Haswell
#   refurb units, AMD Bulldozer/Piledriver) then hit STATUS_ILLEGAL_INSTRUCTION
#   (0xc000001d) the moment whisper.cpp dispatches into a vectorized kernel.
#
# Baseline target:
#   x86_64 + AVX + FMA + F16C  (Intel Sandy Bridge gen 2 ~2011, AMD Bulldozer ~2011)
#   No AVX2, no AVX-512, no AMX, no AVX-VNNI.
#
# How to opt out:
#   Builders who know their target CPUs support newer ISA can set
#   HANDY_GGML_AVX2=ON / HANDY_GGML_AVX512=ON before invoking the build.

if(NOT DEFINED ENV{HANDY_GGML_NATIVE})
  set(GGML_NATIVE OFF CACHE BOOL "Handy: pin to portable baseline, not -march=native" FORCE)
endif()

if(NOT DEFINED ENV{HANDY_GGML_AVX})
  set(GGML_AVX ON CACHE BOOL "Handy baseline: AVX (~2011+)" FORCE)
endif()
if(NOT DEFINED ENV{HANDY_GGML_AVX2})
  set(GGML_AVX2 OFF CACHE BOOL "Handy baseline: drop AVX2 for older CPU support" FORCE)
endif()
if(NOT DEFINED ENV{HANDY_GGML_FMA})
  set(GGML_FMA ON CACHE BOOL "Handy baseline: FMA (~2013+ Intel, ~2011+ AMD)" FORCE)
endif()
if(NOT DEFINED ENV{HANDY_GGML_F16C})
  set(GGML_F16C ON CACHE BOOL "Handy baseline: F16C (~2012+)" FORCE)
endif()
if(NOT DEFINED ENV{HANDY_GGML_AVX512})
  set(GGML_AVX512 OFF CACHE BOOL "Handy baseline: no AVX-512" FORCE)
endif()
if(NOT DEFINED ENV{HANDY_GGML_AVX512_VBMI})
  set(GGML_AVX512_VBMI OFF CACHE BOOL "Handy baseline: no AVX-512-VBMI" FORCE)
endif()
if(NOT DEFINED ENV{HANDY_GGML_AVX512_VNNI})
  set(GGML_AVX512_VNNI OFF CACHE BOOL "Handy baseline: no AVX-512-VNNI" FORCE)
endif()
if(NOT DEFINED ENV{HANDY_GGML_AVX512_BF16})
  set(GGML_AVX512_BF16 OFF CACHE BOOL "Handy baseline: no AVX-512-BF16" FORCE)
endif()
if(NOT DEFINED ENV{HANDY_GGML_AMX_TILE})
  set(GGML_AMX_TILE OFF CACHE BOOL "Handy baseline: no AMX" FORCE)
endif()
if(NOT DEFINED ENV{HANDY_GGML_AMX_INT8})
  set(GGML_AMX_INT8 OFF CACHE BOOL "Handy baseline: no AMX" FORCE)
endif()
if(NOT DEFINED ENV{HANDY_GGML_AMX_BF16})
  set(GGML_AMX_BF16 OFF CACHE BOOL "Handy baseline: no AMX" FORCE)
endif()

message(STATUS "[Handy] ggml CPU baseline pinned: NATIVE=OFF AVX=ON AVX2=OFF FMA=ON F16C=ON AVX512=OFF")
