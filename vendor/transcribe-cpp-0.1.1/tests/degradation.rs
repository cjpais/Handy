//! Backend degradation + dynamic-posture contract.
//!
//! These are no-model tests, so they run in EVERY CI posture — the static
//! default, the `shared` (shared-link) leg, and the `dynamic-backends`
//! (loadable-module) leg of the posture matrix (`rust-ci.yml`). In the
//! shared/dynamic-backends legs they are also the end-to-end proof that a
//! consumer loads and calls into `libtranscribe` (resolved via the rpath the
//! safe crate's build.rs emits), and that the backend modules register.
//!
//! They cover the REQUEST-PATH half of the degradation contract
//! (requirements §5): CPU is always the floor, an unavailable backend probes
//! cleanly (the hook that turns an explicit `Backend::Vulkan` into a clear
//! error rather than a crashed model load), and `init_backends` — the
//! provider/DL entry point — is callable without tearing down the floor. The
//! Vulkan-loader-absent three-tier (no loader / loader-no-device / real GPU) is
//! certified once at the C level (native-ci `provider-dl-vulkan`) and inherited
//! per requirements §4.
//!
//! Posture note: in a `dynamic-backends` build NO backend is compiled in — the
//! CPU floor only exists after the modules are loaded. So every test that
//! asserts the floor first calls [`init_backends_default`], which loads the
//! build's module directory in that posture and is a harmless no-op otherwise.

use transcribe_cpp::{
    backend_available, device_count, devices, init_backends, init_backends_default, Backend,
};

#[test]
fn cpu_is_the_floor() {
    // Establish the floor in every posture (no-op for compiled-in builds; loads
    // the modules for a dynamic-backends build), then assert it holds.
    init_backends_default().expect("init_backends_default");
    assert!(device_count() >= 1, "no compute devices registered");
    assert!(backend_available(Backend::Cpu), "CPU backend unavailable");
    assert!(
        devices().iter().any(|d| d.kind == "cpu"),
        "no cpu-kind device in {:?}",
        devices()
    );
}

#[test]
fn unavailable_backend_probes_cleanly() {
    init_backends_default().expect("init_backends_default");
    // In a CPU/Metal build Vulkan and CUDA are not compiled in. The probe must
    // answer (true/false) without crashing — this is what lets a host turn an
    // explicit Backend::Vulkan request into a clear error instead of a failed
    // model load. We assert it does not panic and is internally consistent;
    // CPU is the one backend we can assert positively on every runner.
    let _vulkan = backend_available(Backend::Vulkan);
    let _cuda = backend_available(Backend::Cuda);
    assert!(backend_available(Backend::Cpu));
}

#[test]
fn init_backends_keeps_the_cpu_floor() {
    // The provider entry point. Establish the floor first (so this holds in a
    // dynamic-backends build), then point init_backends at an EMPTY directory:
    // the contract is that it neither panics nor tears down the already-
    // registered floor. Mirrors link_smoke.c's init_backends call.
    init_backends_default().expect("init_backends_default");
    let dir = std::env::temp_dir().join("transcribe-rs-empty-modules");
    std::fs::create_dir_all(&dir).ok();

    let before = device_count();
    let _ = init_backends(&dir); // posture-dependent result; must not panic
    assert!(
        device_count() >= before.max(1),
        "device count regressed after init_backends ({before} -> {})",
        device_count()
    );
    assert!(backend_available(Backend::Cpu), "CPU floor lost");
}

/// In a `dynamic-backends` build, the default loader must actually register a
/// CPU device from the on-disk modules — proving the loadable-module path works
/// end to end from Rust (the build configured BACKEND_DL, the modules installed,
/// and init_backends found and dlopened them). In other postures this path is a
/// no-op, so the assertion only carries signal under the feature.
#[cfg(feature = "dynamic-backends")]
#[test]
fn dynamic_backends_register_a_cpu_module() {
    init_backends_default().expect("dynamic-backends provider load");
    assert!(
        devices().iter().any(|d| d.kind == "cpu"),
        "dynamic-backends build registered no cpu module: {:?}",
        devices()
    );
}
