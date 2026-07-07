//! No-model tests: version/ABI introspection, the version gate, device
//! discovery, and error mapping. These always run (including on forks).

mod common;

use transcribe_cpp::{
    abi_struct_size, backend_available, compiled_version, device_count, devices, header_hash,
    init_backends_default, version, AbiStruct, Backend, DeviceType, Error, Model,
};

#[test]
fn version_gate_agrees() {
    // The pre-1.0 base-version lock: the linked library and the generated
    // bindings must report the same MAJOR.MINOR.PATCH.
    assert_eq!(version(), compiled_version());
    assert!(!version().is_empty());
}

#[test]
fn header_hash_is_pinned() {
    // sha256/16 over the FFI surface — 16 hex chars.
    let h = header_hash();
    assert_eq!(h.len(), 16, "hash: {h}");
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn abi_struct_sizes_are_live() {
    // Real calls into the native library returning runtime layout values.
    for which in [
        AbiStruct::RunParams,
        AbiStruct::Capabilities,
        AbiStruct::Segment,
        AbiStruct::SessionLimits,
    ] {
        assert!(abi_struct_size(which) > 0, "{which:?} reported size 0");
    }
}

#[test]
fn at_least_a_cpu_device() {
    // A compiled-in build has the CPU backend registered already; a
    // dynamic-backends build registers it once the modules load. Establish it in
    // every posture, then assert the floor.
    init_backends_default().expect("init_backends_default");
    assert!(device_count() >= 1);
    assert!(backend_available(Backend::Cpu));
    let devices = devices();
    assert!(devices.iter().any(|d| d.kind == "cpu"), "{devices:?}");
}

#[test]
fn devices_is_non_empty() {
    // Every build has at least the CPU device registered after backend init.
    init_backends_default().expect("init_backends_default");
    let devices = devices();
    assert!(!devices.is_empty(), "devices() returned empty");
}

#[test]
fn device_fields_are_well_formed() {
    // Walk every enumerated device and check the invariants the binding
    // promises: the registry index matches its position, the device-type axis
    // is consistent for a CPU device, and the string fields are well-formed
    // (device_id is None or non-empty; name/kind are never empty).
    init_backends_default().expect("init_backends_default");
    let devices = devices();
    for (i, dev) in devices.iter().enumerate() {
        // Enumerated devices carry their registry index = position.
        assert_eq!(dev.index, Some(i), "device {i}: {dev:?}");

        // name / kind are always populated.
        assert!(!dev.name.is_empty(), "device {i} empty name: {dev:?}");
        assert!(!dev.kind.is_empty(), "device {i} empty kind: {dev:?}");

        // device_id is either absent or a non-empty stable id.
        if let Some(id) = &dev.device_id {
            assert!(!id.is_empty(), "device {i} empty device_id: {dev:?}");
        }

        // The "cpu" kind must report the CPU device-type (the one cross-build
        // invariant we can assert without depending on present hardware).
        if dev.kind == "cpu" {
            assert_eq!(
                dev.device_type,
                DeviceType::Cpu,
                "cpu-kind device {i} not DeviceType::Cpu: {dev:?}"
            );
        }
    }
}

#[test]
fn missing_file_is_not_found() {
    // Register backends first so a dynamic-backends build reaches the file check
    // (a zero-device load would otherwise fail on the backend, not the path).
    init_backends_default().expect("init_backends_default");
    let err = Model::load("/no/such/model.gguf").unwrap_err();
    assert!(
        matches!(err, Error::ModelFileNotFound(_)),
        "got {err:?} (status {})",
        err.raw_status()
    );
}

#[test]
fn junk_file_is_model_load_error() {
    init_backends_default().expect("init_backends_default");
    let dir = std::env::temp_dir();
    let path = dir.join("transcribe-rs-junk.gguf");
    std::fs::write(&path, b"not a gguf file at all").unwrap();
    let err = Model::load(&path).unwrap_err();
    let _ = std::fs::remove_file(&path);
    assert!(
        matches!(err, Error::ModelLoad(_)),
        "got {err:?} (status {})",
        err.raw_status()
    );
}

#[test]
fn handles_are_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    fn assert_send<T: Send>() {}
    assert_send_sync::<Model>();
    assert_send::<transcribe_cpp::Session>();
    // Session is intentionally NOT Sync (single-threaded use).
}
