// transcribe-env.cpp - see transcribe-env.h.

#include "transcribe-env.h"

#include <cstdlib>

namespace transcribe::env {

const char * str(const char * name) {
    const char * v = std::getenv(name);
    if (v == nullptr || v[0] == '\0') {
        return nullptr;
    }
    return v;
}

bool flag(const char * name) {
    const char * v = str(name);
    return v != nullptr && v[0] != '0';
}

}  // namespace transcribe::env
