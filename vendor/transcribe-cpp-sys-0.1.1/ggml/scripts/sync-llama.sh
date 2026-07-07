#!/bin/bash

cp -rpv ../llama.cpp/ggml/CMakeLists.txt       CMakeLists.txt
cp -rpv ../llama.cpp/ggml/src/CMakeLists.txt   src/CMakeLists.txt

cp -rpv ../llama.cpp/ggml/cmake/*              cmake/
cp -rpv ../llama.cpp/ggml/src/ggml-cpu/cmake/* src/ggml-cpu/cmake/

cp -rpv ../llama.cpp/ggml/src/ggml* src/
cp -rpv ../llama.cpp/ggml/src/gguf* src/

cp -rpv ../llama.cpp/ggml/include/ggml*.h include/
cp -rpv ../llama.cpp/ggml/include/gguf*.h include/

cp -rpv ../llama.cpp/tests/test-opt.cpp           tests/test-opt.cpp
cp -rpv ../llama.cpp/tests/test-quantize-fns.cpp  tests/test-quantize-fns.cpp
cp -rpv ../llama.cpp/tests/test-quantize-perf.cpp tests/test-quantize-perf.cpp
cp -rpv ../llama.cpp/tests/test-backend-ops.cpp   tests/test-backend-ops.cpp

cp -rpv ../llama.cpp/LICENSE                ./LICENSE
cp -rpv ../llama.cpp/scripts/gen-authors.sh ./scripts/gen-authors.sh
