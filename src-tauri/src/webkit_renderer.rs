//! Linux WebKitGTK renderer selection.
//!
//! WebKitGTK 2.54 composites only the damaged region of every frame. With the
//! accelerated compositor replaced by the plain shared-memory path
//! (`WEBKIT_DISABLE_DMABUF_RENDERER=1`, the long-standing workaround for GPU
//! crashes — tauri-apps/tauri#9394) each partial frame arrives in a freshly
//! allocated buffer, so every pixel outside the damage is transparent. For a
//! frameless card like Handy's recording overlay only the animated bits (dot,
//! waveform) survive, which reads as a half-transparent ghost overlay.
//!
//! Keeping the compositor and moving its swap chain to shared memory
//! (`WEBKIT_DMABUF_RENDERER_FORCE_SHM=1`) still bypasses DMA-BUF imports, but
//! paints full frames again. The switch exists since WebKitGTK 2.44; older
//! releases predate both it and the damage tracking, so they keep the previous
//! DMA-BUF-disabled workaround.
//!
//! Explicit user configuration always wins: setting either variable in the
//! environment (shell, desktop entry, Nix wrapper) skips this policy.

/// Applies the WebKitGTK renderer policy for the running WebKit version.
///
/// Must run before GTK/WebKit is initialized. No-op outside Linux.
pub fn apply() {
    #[cfg(target_os = "linux")]
    apply_linux();
}

#[cfg(target_os = "linux")]
fn apply_linux() {
    // Preserve explicit overrides (#2166): users pick their value, and
    // packagers (e.g. the Nix wrapper) can pin the renderer themselves.
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_some()
        || std::env::var_os("WEBKIT_DMABUF_RENDERER_FORCE_SHM").is_some()
    {
        return;
    }

    let (major, minor, _micro) = webkit_version();
    if (major, minor) >= (2, 54) {
        // 2.54 repaints only damage, which loses transparent pixels on the
        // non-composited path; keep the compositor on shared memory instead.
        std::env::set_var("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1");
        log::debug!("webkit renderer: {major}.{minor} -> WEBKIT_DMABUF_RENDERER_FORCE_SHM=1");
    } else {
        // Before the damage tracking the non-composited path is the
        // crash-safe one, so keep the original workaround.
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        log::debug!("webkit renderer: {major}.{minor} -> WEBKIT_DISABLE_DMABUF_RENDERER=1");
    }
}

/// Version of the WebKitGTK library linked into the process.
#[cfg(target_os = "linux")]
fn webkit_version() -> (u32, u32, u32) {
    // SAFETY: the accessors only read version constants compiled into the
    // WebKitGTK library that wry already links; they neither initialize GTK
    // nor touch any other state.
    unsafe {
        (
            webkit2gtk_sys::webkit_get_major_version(),
            webkit2gtk_sys::webkit_get_minor_version(),
            webkit2gtk_sys::webkit_get_micro_version(),
        )
    }
}
