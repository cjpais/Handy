# Shared helper: classify ggml's backend target list into the collapsed
# backend-kind list every artifact contract uses (provider descriptor,
# contract.json, transcribe-link.json).
#
# transcribe_backend_kinds(<out_kinds> <out_targets>)
#   <out_kinds>:   collapsed kind names — "ggml-" prefix stripped, every CPU
#                  ISA variant (ggml-cpu-haswell, ...) collapsed to "cpu",
#                  accelerated kinds first, cpu last (readability only;
#                  ranking is the consumer's job).
#   <out_targets>: the GGML_AVAILABLE_BACKENDS entries that exist as targets
#                  (the libraries/modules an install or wheel must carry).
#
# Reads ggml's GGML_AVAILABLE_BACKENDS internal cache, maintained as backends
# register. Call after add_subdirectory(ggml).
#
# IMPORTANT: that cache is APPEND-ONLY across reconfigures (ggml never
# resets it), so a long-lived build tree that once enabled a backend keeps
# the entry forever. Everything here is therefore filtered to entries that
# exist as TARGETS in the CURRENT configure — kinds included — so a stale
# cache can never stamp a backend into a contract that the artifact does
# not actually contain. (Found the hard way: a local build dir reported
# "blas" with no libggml-blas in the install.)

function(transcribe_backend_kinds out_kinds out_targets)
    set(_targets "")
    set(_kinds "")
    foreach(_backend IN LISTS GGML_AVAILABLE_BACKENDS)
        if(NOT TARGET ${_backend})
            continue()
        endif()
        list(APPEND _targets ${_backend})
        string(REGEX REPLACE "^ggml-" "" _kind "${_backend}")
        string(REGEX REPLACE "^cpu-.*$" "cpu" _kind "${_kind}")
        list(APPEND _kinds "${_kind}")
    endforeach()
    list(REMOVE_DUPLICATES _kinds)

    set(_ordered "")
    foreach(_kind IN ITEMS cuda metal vulkan)
        if(_kind IN_LIST _kinds)
            list(APPEND _ordered "${_kind}")
            list(REMOVE_ITEM _kinds "${_kind}")
        endif()
    endforeach()
    list(REMOVE_ITEM _kinds cpu)
    list(APPEND _ordered ${_kinds} cpu)

    set(${out_kinds} "${_ordered}" PARENT_SCOPE)
    set(${out_targets} "${_targets}" PARENT_SCOPE)
endfunction()
