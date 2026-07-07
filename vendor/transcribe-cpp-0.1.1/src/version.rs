//! Version + ABI introspection and the load-time version gate.
//!
//! Pre-1.0 the on-disk ABI may break between minor releases, so the binding
//! and the linked library must agree on the base `MAJOR.MINOR.PATCH`. The gate
//! runs once, lazily, on the first model load (and is exposed directly so a
//! host can check up front). Packaging-only suffixes on the runtime string are
//! tolerated — only the leading release segment is compared.

use std::sync::OnceLock;

use transcribe_cpp_sys as sys;

use crate::error::{Error, Result};
use crate::result::owned_str;
use crate::types::AbiStruct;

/// The base version string this crate's bindings were built against.
///
/// Taken from this crate's own `Cargo.toml` (`CARGO_PKG_VERSION`), not the
/// generated FFI macros: a version-only bump must not churn the committed
/// bindings or the abihash (notes/releasing.md §8 P0 #1). The generators no
/// longer emit `TRANSCRIBE_VERSION_*`, so this is also the only source left.
pub fn compiled_version() -> String {
    base(env!("CARGO_PKG_VERSION")).to_string()
}

/// The `MAJOR.MINOR.PATCH` version string of the linked native library.
pub fn version() -> String {
    owned_str(unsafe { sys::transcribe_version() })
}

/// The short git commit the native library was built from (or "unknown").
pub fn version_commit() -> String {
    owned_str(unsafe { sys::transcribe_version_commit() })
}

/// The public-ABI digest the committed bindings were generated against.
pub fn header_hash() -> &'static str {
    sys::PUBLIC_HEADER_HASH
}

/// The native library's `sizeof` for a public ABI struct, or 0 if unknown.
pub fn abi_struct_size(which: AbiStruct) -> usize {
    unsafe { sys::transcribe_abi_struct_size(which.to_raw()) }
}

/// The native library's `alignof` for a public ABI struct, or 0 if unknown.
pub fn abi_struct_align(which: AbiStruct) -> usize {
    unsafe { sys::transcribe_abi_struct_align(which.to_raw()) }
}

/// Leading dotted-numeric release segment ("0.0.1.post1" -> "0.0.1").
fn base(v: &str) -> &str {
    let end = v
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(v.len());
    v[..end].trim_end_matches('.')
}

static GATE: OnceLock<std::result::Result<(), String>> = OnceLock::new();

/// Run (once) the pre-1.0 base-version lock against the loaded library.
pub(crate) fn ensure_compatible() -> Result<()> {
    let outcome = GATE.get_or_init(|| {
        let runtime = version();
        let compiled = compiled_version();
        if base(&runtime) == base(&compiled) {
            Ok(())
        } else {
            Err(format!(
                "loaded transcribe library is {runtime}, but these bindings \
                 were generated for {compiled} (base versions must match \
                 pre-1.0)"
            ))
        }
    });
    outcome.clone().map_err(Error::VersionMismatch)
}
