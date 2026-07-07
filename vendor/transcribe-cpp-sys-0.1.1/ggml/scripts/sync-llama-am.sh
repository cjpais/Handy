#!/bin/bash
#
# Synchronize llama.cpp changes to ggml
#
# Usage:
#
#   $ cd /path/to/ggml
#   $ ./scripts/sync-llama-am.sh -skip hash0,hash1,hash2... -C 3
#

set -e

sd=$(dirname $0)
cd $sd/../

SRC_GGML=$(pwd)
SRC_LLAMA=$(cd ../llama.cpp; pwd)

if [ ! -d $SRC_LLAMA ]; then
    echo "llama.cpp not found at $SRC_LLAMA"
    exit 1
fi

lc=$(cat $SRC_GGML/scripts/sync-llama.last)
echo "Syncing llama.cpp changes since commit $lc"

to_skip=""

# context for git patches in number of lines
ctx="8"

while [ "$1" != "" ]; do
    case $1 in
        -skip )
            shift
            to_skip=$1
            ;;
        -C )
            shift
            ctx=$1
            ;;
    esac
    shift
done

cd $SRC_LLAMA

git log --oneline $lc..HEAD
git log --oneline $lc..HEAD --reverse | grep -v "(ggml/[0-9]*)" | grep -v "(whisper/[0-9]*)" | cut -d' ' -f1 > $SRC_GGML/llama-commits

if [ ! -s $SRC_GGML/llama-commits ]; then
    rm -v $SRC_GGML/llama-commits
    echo "No new commits"
    exit 0
fi

if [ -f $SRC_GGML/llama-src.patch ]; then
    rm -v $SRC_GGML/llama-src.patch
fi

while read c; do
    if [ -n "$to_skip" ]; then
        if [[ $to_skip == *"$c"* ]]; then
            echo "Skipping $c"
            continue
        fi
    fi

    git format-patch -U${ctx} -k $c~1..$c --stdout -- \
        ggml/CMakeLists.txt \
        ggml/src/CMakeLists.txt \
        ggml/cmake/BuildTypes.cmake \
        ggml/cmake/GitVars.cmake \
        ggml/cmake/common.cmake \
        ggml/cmake/ggml-config.cmake.in \
        ggml/src/ggml-cpu/cmake/FindSIMD.cmake \
        ggml/src/ggml* \
        ggml/src/gguf* \
        ggml/include/ggml*.h \
        ggml/include/gguf*.h \
        tests/test-opt.cpp \
        tests/test-quantize-fns.cpp \
        tests/test-quantize-perf.cpp \
        tests/test-backend-ops.cpp \
        LICENSE \
        scripts/gen-authors.sh \
        >> $SRC_GGML/llama-src.patch
done < $SRC_GGML/llama-commits

rm -v $SRC_GGML/llama-commits

# delete files if empty
if [ ! -s $SRC_GGML/llama-src.patch ]; then
    rm -v $SRC_GGML/llama-src.patch
fi

cd $SRC_GGML

if [ -f $SRC_GGML/llama-src.patch ]; then
    # replace PR numbers
    #
    # Subject: some text (#1234)
    # Subject: some text (llama/1234)
    cat llama-src.patch | sed -e 's/^Subject: \(.*\) (#\([0-9]*\))/Subject: \1 (llama\/\2)/' > llama-src.patch.tmp
    mv llama-src.patch.tmp llama-src.patch

    cat llama-src.patch | sed -e 's/^\(.*\) (#\([0-9]*\))$/\1 (llama\/\2)/' > llama-src.patch.tmp
    mv llama-src.patch.tmp llama-src.patch

    # replace filenames:
    #
    # ggml/CMakelists.txt       -> CMakeLists.txt
    # ggml/src/CMakelists.txt   -> src/CMakeLists.txt
    #
    # ggml/cmake/BuildTypes.cmake            -> cmake/BuildTypes.cmake
    # ggml/cmake/GitVars.cmake               -> cmake/GitVars.cmake
    # ggml/cmake/common.cmake                -> cmake/common.cmake
    # ggml/cmake/ggml-config.cmake.in        -> cmake/ggml-config.cmake.in
    # ggml/src/ggml-cpu/cmake/FindSIMD.cmake -> src/ggml-cpu/cmake/FindSIMD.cmake
    #
    # ggml/src/ggml* -> src/ggml*
    # ggml/src/gguf* -> src/gguf*
    #
    # ggml/include/ggml*.h -> include/ggml*.h
    # ggml/include/gguf*.h -> include/gguf*.h
    #
    # tests/test-opt.cpp           -> tests/test-opt.cpp
    # tests/test-quantize-fns.cpp  -> tests/test-quantize-fns.cpp
    # tests/test-quantize-perf.cpp -> tests/test-quantize-perf.cpp
    # tests/test-backend-ops.cpp   -> tests/test-backend-ops.cpp
    #
    # LICENSE                -> LICENSE
    # scripts/gen-authors.sh -> scripts/gen-authors.sh

    cat llama-src.patch | sed -E \
        -e 's/(^[[:space:]]| [ab]\/)ggml\/CMakeLists\.txt/\1CMakeLists.txt/g' \
        -e 's/(^[[:space:]]| [ab]\/)ggml\/src\/CMakeLists\.txt/\1src\/CMakeLists.txt/g' \
        -e 's/(^[[:space:]]| [ab]\/)ggml\/cmake\/BuildTypes\.cmake/\1cmake\/BuildTypes\.cmake/g' \
        -e 's/(^[[:space:]]| [ab]\/)ggml\/cmake\/GitVars\.cmake/\1cmake\/GitVars\.cmake/g' \
        -e 's/(^[[:space:]]| [ab]\/)ggml\/cmake\/common\.cmake/\1cmake\/common\.cmake/g' \
        -e 's/(^[[:space:]]| [ab]\/)ggml\/cmake\/ggml-config\.cmake\.in/\1cmake\/ggml-config\.cmake\.in/g' \
        -e 's/(^[[:space:]]| [ab]\/)ggml\/src\/ggml-cpu\/cmake\/FindSIMD\.cmake/\1src\/ggml-cpu\/cmake\/FindSIMD\.cmake/g' \
        -e 's/(^[[:space:]]| [ab]\/)ggml\/src\/ggml(.*)/\1src\/ggml\2/g' \
        -e 's/(^[[:space:]]| [ab]\/)ggml\/src\/gguf(.*)/\1src\/gguf\2/g' \
        -e 's/(^[[:space:]]| [ab]\/)ggml\/include\/ggml(.*)\.h/\1include\/ggml\2.h/g' \
        -e 's/(^[[:space:]]| [ab]\/)ggml\/include\/gguf(.*)\.h/\1include\/gguf\2.h/g' \
        -e 's/(^[[:space:]]| [ab]\/)tests\/test-opt\.cpp/\1tests\/test-opt.cpp/g' \
        -e 's/(^[[:space:]]| [ab]\/)tests\/test-quantize-fns\.cpp/\1tests\/test-quantize-fns.cpp/g' \
        -e 's/(^[[:space:]]| [ab]\/)tests\/test-quantize-perf\.cpp/\1tests\/test-quantize-perf.cpp/g' \
        -e 's/(^[[:space:]]| [ab]\/)tests\/test-backend-ops\.cpp/\1tests\/test-backend-ops.cpp/g' \
        -e 's/(^[[:space:]]| [ab]\/)LICENSE/\1LICENSE/g' \
        -e 's/(^[[:space:]]| [ab]\/)scripts\/gen-authors\.sh/\1scripts\/gen-authors.sh/g' \
        > llama-src.patch.tmp
    mv llama-src.patch.tmp llama-src.patch

    git am -C${ctx} llama-src.patch

    rm -v $SRC_GGML/llama-src.patch
fi

# update last commit
cd $SRC_LLAMA
git log -1 --format=%H > $SRC_GGML/scripts/sync-llama.last

echo "Done"

exit 0
