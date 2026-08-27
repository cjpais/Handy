//! glibc allocator tuning (Linux only).
//!
//! Each dictation allocates multi-megabyte transient buffers — the captured
//! 16 kHz PCM (~64 KB per second, held twice: once for transcription, once
//! for the history WAV) plus the transcription engine's per-run mel/FFT
//! scratch (~80 KB per second of audio) — all freed within seconds.
//!
//! glibc's malloc serves allocations above its "mmap threshold" with a
//! private mmap that is returned to the OS on free. But the threshold is
//! *dynamic*: freeing an mmapped block raises it to that block's size (up to
//! 32 MB), so after the first dictation every later large buffer is served
//! from malloc arenas instead. Arena memory freed by the app is cached for
//! reuse, and interleaved small live allocations pin those pages, so the OS
//! never gets them back: RSS grows by roughly the transient-buffer volume of
//! each dictation, is never touched again, and slowly migrates to swap
//! (issue #1792 — measured at ~15 MB retained per 2-minute dictation;
//! pinning the threshold reduced that to ~0.5 MB).
//!
//! Both entry points are no-ops on non-glibc targets (Windows, macOS, musl):
//! this failure mode is specific to glibc's dynamic-threshold heuristic.

/// Pin glibc's mmap threshold so large transient buffers keep taking the
/// mmap path and are returned to the OS as soon as they are freed.
///
/// Must run before the workload allocates (called at the top of `run()`);
/// the cost is an mmap/munmap round-trip per multi-MB buffer, which is
/// negligible at dictation frequency.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub fn init_allocator() {
    // SAFETY: FFI call with no memory arguments; mallopt only updates
    // malloc's internal parameters.
    unsafe {
        libc::mallopt(libc::M_MMAP_THRESHOLD, 128 * 1024);
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
pub fn init_allocator() {}

/// Return freed-but-cached malloc arena memory to the OS.
///
/// Called once per finished transcription pipeline (see `FinishGuard`); it
/// sweeps whatever smaller-than-threshold churn still accumulates in the
/// arenas. Takes on the order of a millisecond, off the main thread.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub fn trim_freed_memory() {
    // SAFETY: FFI call with no memory arguments; malloc_trim releases whole
    // free pages back to the OS via madvise and is thread-safe.
    unsafe {
        libc::malloc_trim(0);
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
pub fn trim_freed_memory() {}
