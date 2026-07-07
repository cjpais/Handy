#!/bin/bash
#
# Synchronize whisper.cpp changes to ggml
#
# Usage:
#
#   $ cd /path/to/ggml
#   $ ./scripts/sync-whisper-am.sh -skip hash0,hash1,hash2...
#

set -e

sd=$(dirname $0)
cd $sd/../

SRC_GGML=$(pwd)
SRC_WHISPER=$(cd ../whisper.cpp; pwd)

if [ ! -d $SRC_WHISPER ]; then
    echo "whisper.cpp not found at $SRC_WHISPER"
    exit 1
fi

lc=$(cat $SRC_GGML/scripts/sync-whisper.last)
echo "Syncing whisper.cpp changes since commit $lc"

to_skip=""
if [ "$1" == "-skip" ]; then
    to_skip=$2
fi

cd $SRC_WHISPER

git log --oneline $lc..HEAD
git log --oneline $lc..HEAD --reverse | grep -v "(ggml/[0-9]*)" | grep -v "(llama/[0-9]*)" | cut -d' ' -f1 > $SRC_GGML/whisper-commits

if [ ! -s $SRC_GGML/whisper-commits ]; then
    rm -v $SRC_GGML/whisper-commits
    echo "No new commits"
    exit 0
fi

if [ -f $SRC_GGML/whisper-src.patch ]; then
    rm -v $SRC_GGML/whisper-src.patch
fi

while read c; do
    if [ -n "$to_skip" ]; then
        if [[ $to_skip == *"$c"* ]]; then
            echo "Skipping $c"
            continue
        fi
    fi

    git format-patch -k $c~1..$c --stdout -- \
        ggml/CMakeLists.txt \
        ggml/src/CMakeLists.txt \
        ggml/cmake/FindSIMD.cmake \
        ggml/src/ggml* \
        ggml/src/gguf* \
        ggml/include/ggml*.h \
        ggml/include/gguf*.h \
        examples/common-ggml.h \
        examples/common-ggml.cpp \
        LICENSE \
        scripts/gen-authors.sh \
        >> $SRC_GGML/whisper-src.patch
done < $SRC_GGML/whisper-commits

rm -v $SRC_GGML/whisper-commits

# delete files if empty
if [ ! -s $SRC_GGML/whisper-src.patch ]; then
    rm -v $SRC_GGML/whisper-src.patch
fi

cd $SRC_GGML

if [ -f $SRC_GGML/whisper-src.patch ]; then
    # replace PR numbers
    #
    # Subject: some text (#1234)
    # Subject: some text (whisper/1234)
    cat whisper-src.patch | sed -e 's/^Subject: \(.*\) (#\([0-9]*\))/Subject: \1 (whisper\/\2)/' > whisper-src.patch.tmp
    mv whisper-src.patch.tmp whisper-src.patch

    cat whisper-src.patch | sed -e 's/^\(.*\) (#\([0-9]*\))$/\1 (whisper\/\2)/' > whisper-src.patch.tmp
    mv whisper-src.patch.tmp whisper-src.patch

    # replace filenames:
    #
    # ggml/CMakelists.txt       -> CMakeLists.txt
    # ggml/src/CMakelists.txt   -> src/CMakeLists.txt
    # ggml/cmake/FindSIMD.cmake -> cmake/FindSIMD.cmake
    #
    # ggml/src/ggml* -> src/ggml*
    # ggml/src/gguf* -> src/gguf*
    #
    # ggml/include/ggml*.h -> include/ggml*.h
    # ggml/include/gguf*.h -> include/gguf*.h
    #
    # examples/common.h        -> examples/common.h
    # examples/common.cpp      -> examples/common.cpp
    # examples/common-ggml.h   -> examples/common-ggml.h
    # examples/common-ggml.cpp -> examples/common-ggml.cpp
    #
    # LICENSE                -> LICENSE
    # scripts/gen-authors.sh -> scripts/gen-authors.sh

    cat whisper-src.patch | sed -E \
        -e 's/\/ggml\/CMakeLists\.txt/\/CMakeLists.txt/g' \
        -e 's/\/ggml\/src\/CMakeLists\.txt/\/src\/CMakeLists.txt/g' \
        -e 's/\/ggml\/cmake\/FindSIMD\.cmake/\/cmake\/FindSIMD.cmake/g' \
        -e 's/\/ggml\/src\/ggml(.*)/\/src\/ggml\1/g' \
        -e 's/\/ggml\/src\/gguf(.*)/\/src\/gguf\1/g' \
        -e 's/\/ggml\/include\/ggml(.*)\.h/\/include\/ggml\1.h/g' \
        -e 's/\/ggml\/include\/gguf(.*)\.h/\/include\/gguf\1.h/g' \
        -e 's/\/examples\/common\.h/\/examples\/common.h/g' \
        -e 's/\/examples\/common\.cpp/\/examples\/common.cpp/g' \
        -e 's/\/examples\/common-ggml\.h/\/examples\/common-ggml.h/g' \
        -e 's/\/examples\/common-ggml\.cpp/\/examples\/common-ggml.cpp/g' \
        -e 's/\/LICENSE/\/LICENSE/g' \
        -e 's/\/scripts\/gen-authors\.sh/\/scripts\/gen-authors.sh/g' \
        > whisper-src.patch.tmp
    mv whisper-src.patch.tmp whisper-src.patch

    git am whisper-src.patch

    rm -v $SRC_GGML/whisper-src.patch
fi

# update last commit
cd $SRC_WHISPER
git log -1 --format=%H > $SRC_GGML/scripts/sync-whisper.last

echo "Done"

exit 0
