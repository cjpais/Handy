//! Build script for `transcribe-cpp-sys`.
//!
//! Source build is the primary (and currently only) path: the `cmake` crate
//! drives the vendored C++ tree with `TRANSCRIBE_INSTALL=ON`, and the link
//! line is reconstructed from NOTHING but the installed
//! `lib/transcribe-link.json` manifest — the same artifact the `link_smoke`
//! CI lane compiles a toy C consumer against. No per-platform link lists are
//! hardcoded here (the whisper-rs drift class this avoids).
//!
//! Cargo features map directly to CMake options:
//!   `shared`           -> TRANSCRIBE_BUILD_SHARED=ON (default: static)
//!   `dynamic-backends` -> the above + TRANSCRIBE_GGML_BACKEND_DL=ON (+ x86:
//!                         GGML_CPU_ALL_VARIANTS=ON, TRANSCRIBE_X86_CONSERVATIVE=ON)
//!   `metal`            -> TRANSCRIBE_METAL=ON   (Apple targets only; no-op elsewhere)
//!   `vulkan`           -> TRANSCRIBE_VULKAN=ON
//!   `cuda`             -> TRANSCRIBE_CUDA=ON
//!   `openmp`           -> TRANSCRIBE_USE_OPENMP=ON
//! Official-artifact hygiene flags (OpenMP/BLAS off) are deliberately NOT
//! forced here: a source build is the consumer's build (same philosophy as
//! the Python sdist).
//!
//! Escape hatch: anything else CMake accepts can be passed via the
//! TRANSCRIBE_CMAKE_ARGS (or CMAKE_ARGS) env var — see the passthrough at the
//! end of main(). This is the "no Cargo feature is a hard ceiling" guarantee.

use std::env;
use std::path::{Path, PathBuf};

fn feature(name: &str) -> bool {
    env::var_os(format!("CARGO_FEATURE_{name}")).is_some()
}

/// Split a CMAKE_ARGS-style string into individual arguments, honoring simple
/// double-quotes so a value containing spaces survives (e.g. `-DFOO="a b"`).
/// Whitespace-separated otherwise; quotes are stripped from the emitted token.
fn split_cmake_args(s: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut has_token = false;
    for c in s.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                has_token = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if has_token {
                    args.push(std::mem::take(&mut cur));
                    has_token = false;
                }
            }
            c => {
                cur.push(c);
                has_token = true;
            }
        }
    }
    if has_token {
        args.push(cur);
    }
    args
}

fn main() {
    // CARGO_MANIFEST_DIR for this crate is the repo root (the sys crate's
    // manifest lives there so the tarball can carry the whole C++ tree).
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    for p in [
        "CMakeLists.txt",
        "CMakePresets.json",
        "include",
        "src",
        "ggml",
        "cmake",
        "bindings/rust/sys/build.rs",
        "bindings/rust/sys/src",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(p).display());
    }

    // `dynamic-backends` (loadable backend modules) requires a shared library,
    // so it implies `shared`. The Cargo manifest already encodes that implication
    // (`dynamic-backends = ["shared"]`), but treat it as load-bearing here too.
    let dynamic_backends = feature("DYNAMIC_BACKENDS");
    let shared = feature("SHARED") || dynamic_backends;
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let is_apple = matches!(target_os.as_str(), "macos" | "ios");
    let is_x86 = matches!(target_arch.as_str(), "x86" | "x86_64");

    let mut cfg = cmake::Config::new(&root);
    cfg.profile("Release") // a transcription library always wants an optimized native core
        .define("TRANSCRIBE_INSTALL", "ON")
        .define("TRANSCRIBE_BUILD_TESTS", "OFF")
        .define("TRANSCRIBE_BUILD_EXAMPLES", "OFF")
        .define("TRANSCRIBE_BUILD_TOOLS", "OFF")
        .define("TRANSCRIBE_BUILD_SHARED", if shared { "ON" } else { "OFF" });

    // Force optimization on MSVC. `.profile("Release")` only selects the *config*
    // (and CRT) of the Visual Studio multi-config generator — it does NOT
    // guarantee optimization. cmake-rs strips `/O*` from the cc-derived flags
    // ("let cmake deal with optimization") and then, for the VS generator,
    // overwrites CMAKE_<LANG>_FLAGS_RELEASE with those stripped flags — dropping
    // CMake's default `/O2 /Ob2 /DNDEBUG`. The "Release" build then compiles ggml
    // UNOPTIMIZED with assertions live: ~7x slower on CPU (realtime collapses to
    // ~1x) and a pred-heavy ~3x slower decode on Vulkan. Flags passed via
    // cflag/cxxflag are injected verbatim (un-stripped) into that same
    // CMAKE_<LANG>_FLAGS_RELEASE — the var the VS generator actually respects — so
    // re-adding them here restores optimization. (Off-MSVC the GNU/Clang lanes get
    // -O from the profile and CMake's Release init flags aren't stripped.)
    if target_env == "msvc" {
        for flag in ["/O2", "/Ob2", "/DNDEBUG"] {
            cfg.cflag(flag);
            cfg.cxxflag(flag);
        }
    }

    // Dynamic backend modules: each compute backend becomes a loadable module
    // next to libtranscribe, picked at runtime by transcribe_init_backends().
    // The root CMakeLists validates BACKEND_DL => SHARED and force-sets
    // GGML_NATIVE=OFF, so this just flips the knobs. On x86, fan the CPU backend
    // out into one module per ISA tier (runtime feature scoring) over the
    // SIGILL-safe x86 floor — the same posture the Linux/Windows cpu-vulkan
    // wheel lane ships. ALL_VARIANTS is an x86 concept; on arm a DL build is a
    // single portable CPU module.
    if dynamic_backends {
        cfg.define("TRANSCRIBE_GGML_BACKEND_DL", "ON");
        if is_x86 {
            cfg.define("GGML_CPU_ALL_VARIANTS", "ON");
            cfg.define("TRANSCRIBE_X86_CONSERVATIVE", "ON");
        }
    }

    // Metal: on Apple, set TRANSCRIBE_METAL EXPLICITLY to track the `metal`
    // feature. CMake defaults TRANSCRIBE_METAL ON on Apple Silicon, so without an
    // explicit OFF a `--no-default-features` (metal off) build would still enable
    // Metal — breaking Cargo.toml's "pure CPU build on macOS is
    // default-features = false" contract. Off Apple the feature is a no-op (CMake
    // already defaults it OFF).
    if is_apple {
        if feature("METAL") {
            cfg.define("TRANSCRIBE_METAL", "ON");
            // Self-contained installed tree: embed the metallib instead of a
            // sidecar default.metallib next to the lib (matches the shipped macOS
            // wheel posture; what the shared-infra link-smoke uses).
            cfg.define("GGML_METAL_EMBED_LIBRARY", "ON");
        } else {
            cfg.define("TRANSCRIBE_METAL", "OFF");
        }
    }
    if feature("VULKAN") {
        cfg.define("TRANSCRIBE_VULKAN", "ON");
    }
    if feature("CUDA") {
        cfg.define("TRANSCRIBE_CUDA", "ON");
    }
    // Keep OpenMP OFF unless explicitly opted in. TRANSCRIBE_USE_OPENMP already
    // defaults OFF in CMake (the native ggml threadpool is the default path); we
    // set it explicitly here so `--features openmp` is the single switch. We keep
    // it OFF by default because OpenMP's `-fopenmp` shows up only as a manifest
    // link_flag → a `cargo:rustc-link-arg` that does NOT propagate to downstream
    // binaries, so a static consumer link would fail with undefined GOMP_*/omp_*
    // symbols. A self-contained static build is the default; `--features openmp`
    // opts in (and then owns providing the OpenMP runtime).
    cfg.define(
        "TRANSCRIBE_USE_OPENMP",
        if feature("OPENMP") { "ON" } else { "OFF" },
    );
    // Windows no longer needs ggml's OpenMP. ggml's native CPU threadpool barrier
    // used to deadlock under MSVC (its spin `relax` compiled to a no-op, starving
    // an un-arrived worker), so OpenMP was the only multi-threaded CPU path there.
    // That barrier is fixed upstream (ggml-cpu.c: YieldProcessor relax + a
    // bounded-spin ggml_thread_yield fallback), so the native pool is correct on
    // MSVC — and avoids the vcomp pool that crashes at teardown when a host
    // dlopen/dlcloses this lib. We therefore leave GGML_OPENMP at its
    // CMakeLists-forced OFF on every platform; `--features openmp` is the only
    // opt-in. See the "OpenMP — CENTRAL POLICY" block in the root CMakeLists.txt.

    // Escape hatch: forward arbitrary configure args so the curated features are
    // never a hard ceiling. Anything CMake accepts (-DGGML_*, a -DTRANSCRIBE_*
    // the features don't cover, a toolchain define, ...) can be passed via
    // TRANSCRIBE_CMAKE_ARGS / CMAKE_ARGS. Fed AFTER the feature-derived defines
    // so a user -D wins on the first configure. Unsupported/untested by design —
    // it exists so a consumer is never blocked. The link line is still
    // reconstructed from the regenerated manifest, so whatever this turns on is
    // linked correctly with no per-flag knowledge here.
    for var in ["TRANSCRIBE_CMAKE_ARGS", "CMAKE_ARGS"] {
        println!("cargo:rerun-if-env-changed={var}");
        if let Ok(extra) = env::var(var) {
            for arg in split_cmake_args(&extra) {
                cfg.configure_arg(arg);
            }
        }
    }

    // Builds + installs into OUT_DIR; the returned path IS the install prefix.
    let prefix = cfg.build();

    let manifest = find_manifest(&prefix)
        .unwrap_or_else(|| panic!("transcribe-link.json not found under {}", prefix.display()));
    emit_link_lines(&prefix, &manifest);
}

/// GNUInstallDirs picks `lib` or `lib64`; find the manifest under either.
fn find_manifest(prefix: &Path) -> Option<PathBuf> {
    for libdir in ["lib", "lib64"] {
        let p = prefix.join(libdir).join("transcribe-link.json");
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn emit_link_lines(prefix: &Path, manifest_path: &Path) {
    let text = std::fs::read_to_string(manifest_path).expect("read transcribe-link.json");
    let json: serde_json::Value = serde_json::from_str(&text).expect("parse transcribe-link.json");

    let strs = |key: &str| -> Vec<String> {
        json[key]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };

    let shared = json["shared"].as_bool().unwrap_or(false);
    let lib_dir = prefix.join(json["lib_dir"].as_str().unwrap_or("lib"));
    println!("cargo:rustc-link-search=native={}", lib_dir.display());

    // Archives (static) or the single shared lib. The manifest order is
    // single-pass-GNU-ld safe (each archive's undefined refs resolve in a
    // later one: transcribe -> ggml -> backends -> ggml-base).
    let kind = if shared { "dylib" } else { "static" };
    for name in strs("libraries") {
        println!("cargo:rustc-link-lib={kind}={name}");
    }

    // Absolute library paths (e.g. a find_package(BLAS) result): link the file.
    for path in strs("library_paths") {
        println!("cargo:rustc-link-arg={path}");
    }
    // System libraries the C++/backend archives drag in.
    for name in strs("system_libs") {
        println!("cargo:rustc-link-lib=dylib={name}");
    }
    // Apple frameworks (Metal/Foundation/Accelerate...).
    for name in strs("frameworks") {
        println!("cargo:rustc-link-lib=framework={name}");
    }
    // Extra link flags (e.g. -fopenmp).
    for flag in strs("link_flags") {
        println!("cargo:rustc-link-arg={flag}");
    }
    // Shared posture: the installed libs carry $ORIGIN/@loader_path rpaths, but
    // the consumer binary still needs to find lib_dir itself. Unix uses an
    // rpath; Windows has no rpath (the DLL resolves via PATH / the exe dir), so
    // nothing is emitted there. NOTE: rustc-link-arg does NOT propagate to
    // downstream crates, so this rpath only reaches THIS crate's own artifacts
    // (the -sys smoke tests). The safe crate re-emits it for its tests/examples
    // — see bindings/rust/transcribe-cpp/build.rs.
    let is_windows = env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    if shared && !is_windows {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_dir.display());
    }
    // Windows has no rpath: a shared-posture binary resolves transcribe.dll +
    // the ggml DLLs from its OWN directory. Stage the installed DLLs next to
    // every artifact cargo produces so tests/examples/bins run in place.
    if shared && is_windows {
        stage_windows_dlls(prefix);
    }

    // Metadata for the safe crate's build script (DEP_TRANSCRIBE_* because this
    // crate sets `links = "transcribe"`). The wrapper re-emits these one more hop
    // as DEP_TRANSCRIBE_CPP_* for downstream packaging — see its build.rs.
    println!(
        "cargo:include_dir={}",
        prefix
            .join(json["include_dir"].as_str().unwrap_or("include"))
            .display()
    );
    println!("cargo:lib_dir={}", lib_dir.display());

    // Runtime-artifact dirs for downstream PACKAGING. Only a shared posture
    // produces separate runtime files to ship next to a consumer's executable;
    // a static build bakes everything into the consumer binary, so these stay
    // unset and the consumer reads their absence as "nothing to bundle".
    if shared {
        let bin_dir = prefix.join("bin");
        // The ONE directory a consumer copies into their installer. Windows keeps
        // the runtime DLLs in bin/; on Unix the .so/.dylib live in lib_dir. A
        // single value so the consumer needs no per-OS cfg — `bin_dir` alone
        // would be too Windows-specific.
        let runtime_dir = if is_windows { &bin_dir } else { &lib_dir };
        println!("cargo:runtime_dir={}", runtime_dir.display());
        println!("cargo:bin_dir={}", bin_dir.display());
        // dynamic-backends: the loadable compute modules (a ggml backend subdir,
        // or bin/ on Windows). The manifest already records the install-relative
        // path; forward it so a consumer bundles the modules too — without them a
        // relocated DL build registers zero compute devices.
        if let Some(rel) = json["module_dir"].as_str() {
            println!("cargo:module_dir={}", prefix.join(rel).display());
        }
    }
}

/// Copy the installed runtime DLLs (`<prefix>/bin/*.dll` — transcribe.dll plus
/// the ggml DLLs) next to every artifact cargo will build, so a shared-posture
/// consumer's tests/examples/bins find them with no rpath (Windows has none)
/// and no PATH fiddling. Windows + `shared` only; a no-op anywhere else.
///
/// Windows resolves a process's DLLs from the EXE's own directory first, and
/// cargo emits tests into `deps/`, examples into `examples/`, and bins into the
/// profile root — so all three get the DLLs. This runs from the -sys build
/// script (it owns the native build), which is always in the dep graph, so it
/// covers the safe crate's artifacts too. Best-effort: copy failures warn
/// rather than fail the build.
fn stage_windows_dlls(prefix: &Path) {
    let bin_dir = prefix.join("bin");
    let dlls: Vec<PathBuf> = match std::fs::read_dir(&bin_dir) {
        Ok(entries) => entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("dll"))
            .collect(),
        Err(_) => Vec::new(),
    };
    if dlls.is_empty() {
        println!(
            "cargo:warning=transcribe-cpp-sys: no DLLs under {} to stage for the shared posture",
            bin_dir.display()
        );
        return;
    }
    // OUT_DIR = <target>/<profile>/build/<crate>-<hash>/out; the profile root is
    // four ancestors up. tests -> deps/, examples -> examples/, bins -> root.
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let Some(profile_dir) = out_dir.ancestors().nth(3) else {
        return;
    };
    for sub in ["", "deps", "examples"] {
        let dest = if sub.is_empty() {
            profile_dir.to_path_buf()
        } else {
            profile_dir.join(sub)
        };
        let _ = std::fs::create_dir_all(&dest);
        for dll in &dlls {
            if let Some(name) = dll.file_name() {
                let _ = std::fs::copy(dll, dest.join(name));
            }
        }
    }
    // Re-stage when the built DLLs change.
    println!("cargo:rerun-if-changed={}", bin_dir.display());
}
