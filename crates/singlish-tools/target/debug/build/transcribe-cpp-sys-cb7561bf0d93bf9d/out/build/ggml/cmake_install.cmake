# Install script for directory: /root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/transcribe-cpp-sys-0.4.1/ggml

# Set the install prefix
if(NOT DEFINED CMAKE_INSTALL_PREFIX)
  set(CMAKE_INSTALL_PREFIX "/home/user/Handy/crates/singlish-tools/target/debug/build/transcribe-cpp-sys-cb7561bf0d93bf9d/out")
endif()
string(REGEX REPLACE "/$" "" CMAKE_INSTALL_PREFIX "${CMAKE_INSTALL_PREFIX}")

# Set the install configuration name.
if(NOT DEFINED CMAKE_INSTALL_CONFIG_NAME)
  if(BUILD_TYPE)
    string(REGEX REPLACE "^[^A-Za-z0-9_]+" ""
           CMAKE_INSTALL_CONFIG_NAME "${BUILD_TYPE}")
  else()
    set(CMAKE_INSTALL_CONFIG_NAME "Release")
  endif()
  message(STATUS "Install configuration: \"${CMAKE_INSTALL_CONFIG_NAME}\"")
endif()

# Set the component getting installed.
if(NOT CMAKE_INSTALL_COMPONENT)
  if(COMPONENT)
    message(STATUS "Install component: \"${COMPONENT}\"")
    set(CMAKE_INSTALL_COMPONENT "${COMPONENT}")
  else()
    set(CMAKE_INSTALL_COMPONENT)
  endif()
endif()

# Install shared libraries without execute permission?
if(NOT DEFINED CMAKE_INSTALL_SO_NO_EXE)
  set(CMAKE_INSTALL_SO_NO_EXE "1")
endif()

# Is this installation the result of a crosscompile?
if(NOT DEFINED CMAKE_CROSSCOMPILING)
  set(CMAKE_CROSSCOMPILING "FALSE")
endif()

# Set default install directory permissions.
if(NOT DEFINED CMAKE_OBJDUMP)
  set(CMAKE_OBJDUMP "/usr/bin/objdump")
endif()

if(NOT CMAKE_INSTALL_LOCAL_ONLY)
  # Include the install script for the subdirectory.
  include("/home/user/Handy/crates/singlish-tools/target/debug/build/transcribe-cpp-sys-cb7561bf0d93bf9d/out/build/ggml/src/cmake_install.cmake")
endif()

if(CMAKE_INSTALL_COMPONENT STREQUAL "Unspecified" OR NOT CMAKE_INSTALL_COMPONENT)
  file(INSTALL DESTINATION "${CMAKE_INSTALL_PREFIX}/lib" TYPE STATIC_LIBRARY FILES "/home/user/Handy/crates/singlish-tools/target/debug/build/transcribe-cpp-sys-cb7561bf0d93bf9d/out/build/ggml/src/libggml.a")
endif()

if(CMAKE_INSTALL_COMPONENT STREQUAL "Unspecified" OR NOT CMAKE_INSTALL_COMPONENT)
  file(INSTALL DESTINATION "${CMAKE_INSTALL_PREFIX}/include" TYPE FILE FILES
    "/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/transcribe-cpp-sys-0.4.1/ggml/include/ggml.h"
    "/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/transcribe-cpp-sys-0.4.1/ggml/include/ggml-cpu.h"
    "/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/transcribe-cpp-sys-0.4.1/ggml/include/ggml-alloc.h"
    "/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/transcribe-cpp-sys-0.4.1/ggml/include/ggml-backend.h"
    "/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/transcribe-cpp-sys-0.4.1/ggml/include/ggml-blas.h"
    "/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/transcribe-cpp-sys-0.4.1/ggml/include/ggml-cann.h"
    "/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/transcribe-cpp-sys-0.4.1/ggml/include/ggml-cpp.h"
    "/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/transcribe-cpp-sys-0.4.1/ggml/include/ggml-cuda.h"
    "/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/transcribe-cpp-sys-0.4.1/ggml/include/ggml-opt.h"
    "/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/transcribe-cpp-sys-0.4.1/ggml/include/ggml-metal.h"
    "/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/transcribe-cpp-sys-0.4.1/ggml/include/ggml-rpc.h"
    "/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/transcribe-cpp-sys-0.4.1/ggml/include/ggml-virtgpu.h"
    "/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/transcribe-cpp-sys-0.4.1/ggml/include/ggml-sycl.h"
    "/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/transcribe-cpp-sys-0.4.1/ggml/include/ggml-vulkan.h"
    "/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/transcribe-cpp-sys-0.4.1/ggml/include/ggml-webgpu.h"
    "/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/transcribe-cpp-sys-0.4.1/ggml/include/ggml-zendnn.h"
    "/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/transcribe-cpp-sys-0.4.1/ggml/include/ggml-openvino.h"
    "/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/transcribe-cpp-sys-0.4.1/ggml/include/gguf.h"
    )
endif()

if(CMAKE_INSTALL_COMPONENT STREQUAL "Unspecified" OR NOT CMAKE_INSTALL_COMPONENT)
  file(INSTALL DESTINATION "${CMAKE_INSTALL_PREFIX}/lib" TYPE STATIC_LIBRARY FILES "/home/user/Handy/crates/singlish-tools/target/debug/build/transcribe-cpp-sys-cb7561bf0d93bf9d/out/build/ggml/src/libggml-base.a")
endif()

if(CMAKE_INSTALL_COMPONENT STREQUAL "Unspecified" OR NOT CMAKE_INSTALL_COMPONENT)
  file(INSTALL DESTINATION "${CMAKE_INSTALL_PREFIX}/lib/cmake/ggml" TYPE FILE FILES
    "/home/user/Handy/crates/singlish-tools/target/debug/build/transcribe-cpp-sys-cb7561bf0d93bf9d/out/build/ggml/ggml-config.cmake"
    "/home/user/Handy/crates/singlish-tools/target/debug/build/transcribe-cpp-sys-cb7561bf0d93bf9d/out/build/ggml/ggml-config-version.cmake"
    )
endif()

