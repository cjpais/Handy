#!/bin/bash

cp -rpv ../whisper.cpp/ggml/CMakeLists.txt       CMakeLists.txt
cp -rpv ../whisper.cpp/ggml/src/CMakeLists.txt   src/CMakeLists.txt
cp -rpv ../whisper.cpp/ggml/cmake/FindSIMD.cmake cmake/FindSIMD.cmake

cp -rpv ../whisper.cpp/ggml/src/ggml* src/
cp -rpv ../whisper.cpp/ggml/src/gguf* src/

cp -rpv ../whisper.cpp/ggml/include/ggml*.h include/
cp -rpv ../whisper.cpp/ggml/include/gguf*.h include/

cp -rpv ../whisper.cpp/examples/common-ggml.h   examples/common-ggml.h
cp -rpv ../whisper.cpp/examples/common-ggml.cpp examples/common-ggml.cpp

cp -rpv ../whisper.cpp/LICENSE                ./LICENSE
cp -rpv ../whisper.cpp/scripts/gen-authors.sh ./scripts/gen-authors.sh
