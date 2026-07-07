# transcribe-cpp

Safe, idiomatic Rust bindings for
[transcribe.cpp](https://github.com/handy-computer/transcribe.cpp), a C/C++
speech-to-text library built on ggml.

> **Status: in development (0.0.1).** Core model, session, run, stream,
> cancellation, backend, and family-extension APIs are implemented and tested.

## Install

```sh
cargo add transcribe-cpp
```

The native library is compiled from source by the `transcribe-cpp-sys` crate, so
a first build needs a C++ toolchain and **CMake**. There is no external
compression dependency (the deflate codec is vendored), so no system zlib /
vcpkg setup is required on any platform. No prebuilt download and no environment
variables on the happy path.

## Quickstart

```rust
use transcribe_cpp::{Model, RunOptions};

let mut session = Model::load("model.gguf")?.session()?;
// pcm: 16 kHz mono f32 in [-1, 1]
let result = session.run(&pcm, &RunOptions::default())?;
println!("{}", result.text);
# Ok::<(), transcribe_cpp::Error>(())
```

Runnable examples:

```sh
cargo run --example transcribe-file
cargo run --example streaming
cargo run --example batch
cargo run --example backend-select
cargo run --example error-handling
```

The raw FFI layer is
[`transcribe-cpp-sys`](https://crates.io/crates/transcribe-cpp-sys); this crate
is the safe wrapper.

## Backends

Backends are selected with cargo features forwarded to `transcribe-cpp-sys`:
`metal` (default on Apple), `vulkan`, `cuda`, and `openmp`.

The default link is static and self-contained. Advanced packaging modes are
available through `shared` and `dynamic-backends`; see the `transcribe-cpp-sys`
README if you need runtime-loaded backend modules or custom
`TRANSCRIBE_CMAKE_ARGS`.

## Packaging a distributable (`shared` / `dynamic-backends`)

With the **default static** build there is nothing to do — the native code is
baked into your binary. But if you enable `shared` or `dynamic-backends`, your
installer must ship the runtime libraries (and, for `dynamic-backends`, the
backend modules) next to your executable. Bundling for distribution (e.g. a
Tauri/Electron installer) happens at *build* time, so a runtime lookup is the
wrong tool — you need the artifact path while you build.

This crate forwards the native build's output directories to **your** build
script as `DEP_TRANSCRIBE_CPP_*` env vars. The one you usually want is
`DEP_TRANSCRIBE_CPP_RUNTIME_DIR`: a single directory holding the shared objects
to copy beside your exe (the DLLs on Windows, the `.so`/`.dylib` on Unix — no
per-OS logic needed). Also available: `DEP_TRANSCRIBE_CPP_LIB_DIR`,
`_BIN_DIR`, `_MODULE_DIR` (the `dynamic-backends` modules), and `_INCLUDE_DIR`.
The runtime keys are present only in a shared posture; in a static build they
are unset (nothing to bundle).

In your application's `build.rs`:

```rust
use std::{env, fs, path::{Path, PathBuf}};

fn main() {
    // Present only for shared / dynamic-backends builds.
    let Some(runtime_dir) = env::var_os("DEP_TRANSCRIBE_CPP_RUNTIME_DIR") else {
        return; // static build — nothing to ship
    };

    // A stable folder your bundler references with a STATIC path (e.g.
    // tauri.conf.json `"resources": { "transcribe-libs/*": "." }`).
    let dest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("transcribe-libs");
    fs::create_dir_all(&dest).unwrap();

    // Match by NAME, not extension: Linux versions its libs (libtranscribe.so.0,
    // .so.0.0.4) and the loader needs the SONAME, so an extension filter would
    // copy only the bare dev symlink and ship a broken installer. `fs::copy`
    // dereferences the version symlinks into real files. Returns the count.
    fn copy_libs(src: &Path, dest: &Path) -> usize {
        let mut n = 0;
        for entry in fs::read_dir(src).unwrap_or_else(|e| panic!("read {}: {e}", src.display())) {
            let path = entry.unwrap().path();
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let is_lib = name.ends_with(".dll")
                || name.ends_with(".dylib")  // macOS versions before the ext: libfoo.0.dylib
                || name.ends_with(".so")
                || name.contains(".so.");    // Linux SONAME/version: libfoo.so.0[.0.4]
            if is_lib {
                fs::copy(&path, dest.join(name)).unwrap();
                n += 1;
            }
        }
        n
    }

    // HARD-FAIL if a dir was advertised but holds no libraries: better to break
    // the build than ship a silently broken installer (missing libs at runtime).
    let runtime_dir = PathBuf::from(runtime_dir);
    assert!(copy_libs(&runtime_dir, &dest) > 0, "no runtime libraries in {}", runtime_dir.display());
    println!("cargo:rerun-if-env-changed=DEP_TRANSCRIBE_CPP_RUNTIME_DIR");

    // dynamic-backends only: also ship the loadable compute modules (often a
    // different directory than the library). Unset in a plain `shared` build.
    println!("cargo:rerun-if-env-changed=DEP_TRANSCRIBE_CPP_MODULE_DIR");
    if let Some(module_dir) = env::var_os("DEP_TRANSCRIBE_CPP_MODULE_DIR") {
        let module_dir = PathBuf::from(module_dir);
        assert!(copy_libs(&module_dir, &dest) > 0, "no backend modules in {}", module_dir.display());
        // At runtime, before the first model load, point the library at the
        // bundled modules: `init_backends(<dir next to your exe>)`, or
        // `init_backends_default()` when they sit right beside the executable.
    }
}
```

## Threading

- `Model` is `Send + Sync` and cheap to clone (`Arc`-backed); the native model
  is freed only after the last handle and every `Session` drop.
- `Session` is `Send` but not `Sync`; mutating calls take `&mut self`.
- In 0.x the C library allows at most one in-flight run across all sessions of a
  model; this crate enforces it with a per-model mutex, so concurrent calls
  queue rather than race. For real parallelism, use one `Model` per worker.

- License: MIT
