//! Build script for the safe `transcribe-cpp` crate.
//!
//! The native build and link live entirely in `transcribe-cpp-sys`. This script
//! does two things, both rooted in the same cargo rule: a dependency's metadata
//! reaches only its IMMEDIATE dependent (us), never a crate that depends on us.
//!
//! 1. **shared-lib rpath.** In the `shared` posture an rpath to the sys crate's
//!    lib dir (`DEP_TRANSCRIBE_LIB_DIR`) lets this crate's own tests/examples
//!    find `libtranscribe` at runtime — `rustc-link-arg` doesn't propagate from
//!    a dependency, so the rpath sys emits for itself doesn't reach here. Windows
//!    has no rpath (DLLs resolve via PATH / the exe dir), so nothing is emitted.
//!
//! 2. **Forward the location metadata one more hop.** A consumer packaging a
//!    distributable (a Tauri/Electron installer) needs the sys crate's output
//!    paths (`DEP_TRANSCRIBE_*`) at BUILD time, but only sees `DEP_*` from its
//!    direct dependency — this crate. So we claim `links = "transcribe_cpp"` and
//!    re-emit each value as `DEP_TRANSCRIBE_CPP_*`. See the README packaging section.
//!
//! In the default static posture the runtime keys are unset (nothing to ship)
//! and the rpath branch is inert; `include_dir`/`lib_dir` still forward.

use std::env;
use std::path::Path;

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    // `shared` is set whenever the lib is a separate file — both the plain
    // `shared` feature and `dynamic-backends` (which enables `shared` here).
    let shared = env::var_os("CARGO_FEATURE_SHARED").is_some();

    // (1) Mirror the sys crate's runtime rpath onto THIS crate's binaries (tests,
    // examples), which the dependency's rustc-link-arg cannot reach.
    if shared && target_os != "windows" {
        if let Some(lib_dir) = env::var_os("DEP_TRANSCRIBE_LIB_DIR") {
            println!(
                "cargo:rustc-link-arg=-Wl,-rpath,{}",
                Path::new(&lib_dir).display()
            );
        }
    }

    // (2) Forward the sys crate's location metadata one more hop (see above).
    // Each key is present only when the build produced it: `runtime_dir`/`bin_dir`
    // need `shared`, `module_dir` needs `dynamic-backends`; `include_dir`/`lib_dir`
    // are always set. The lowercase key becomes DEP_TRANSCRIBE_CPP_<KEY> upstack.
    for key in [
        "include_dir",
        "lib_dir",
        "runtime_dir",
        "bin_dir",
        "module_dir",
    ] {
        let dep_var = format!("DEP_TRANSCRIBE_{}", key.to_uppercase());
        println!("cargo:rerun-if-env-changed={dep_var}");
        if let Some(val) = env::var_os(&dep_var) {
            println!("cargo:{key}={}", Path::new(&val).display());
        }
    }
}
