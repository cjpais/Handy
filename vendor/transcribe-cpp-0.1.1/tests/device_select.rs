//! Model-gated device-selection tests. Exercise the `ModelOptions.gpu_device`
//! registry selector and the `Model::device()` correlation back to `devices()`.
//! Skip cleanly (via the `common::` fixtures) when the canary GGUF is absent.

mod common;

use transcribe_cpp::{devices, Backend, Error, Model, ModelOptions};

#[test]
fn loaded_model_reports_an_enumerated_device() {
    // A model loaded with default options resolves a device. That Device has no
    // registry index (transcribe_model_get_device does not expose one), and it
    // correlates back to a device from devices() by name (+ device_id when set).
    let Some((model_path, _)) = common::smoke_fixtures("loaded_model_reports_an_enumerated_device")
    else {
        return;
    };
    let model = Model::load(&model_path).unwrap();
    let dev = model.device().expect("model.device()");

    // A model-resolved device never carries a registry index.
    assert_eq!(dev.index, None, "{dev:?}");

    // It must match one of the enumerated devices by name, and by device_id
    // when the model device reports one.
    let all = devices();
    let matched = all
        .iter()
        .any(|d| d.name == dev.name && (dev.device_id.is_none() || d.device_id == dev.device_id));
    assert!(
        matched,
        "model device {dev:?} not found among enumerated devices {all:?}"
    );
}

#[test]
fn negative_gpu_device_is_invalid_argument() {
    // gpu_device must be >= 0; -1 is rejected before any device lookup.
    let Some((model_path, _)) = common::smoke_fixtures("negative_gpu_device_is_invalid_argument")
    else {
        return;
    };
    let err = Model::load_with(
        &model_path,
        &ModelOptions {
            gpu_device: -1,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(matches!(err, Error::InvalidArgument(_)), "got {err:?}");
}

#[test]
fn out_of_range_gpu_device_is_invalid_argument() {
    // A registry index well past the device count is out of range.
    let Some((model_path, _)) =
        common::smoke_fixtures("out_of_range_gpu_device_is_invalid_argument")
    else {
        return;
    };
    let out_of_range = devices().len() as i32 + 1000;
    let err = Model::load_with(
        &model_path,
        &ModelOptions {
            gpu_device: out_of_range,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(matches!(err, Error::InvalidArgument(_)), "got {err:?}");
}

#[test]
fn gpu_device_with_cpu_backend_is_invalid_argument() {
    // Selecting a device index under a strict-CPU backend request is a category
    // error (there is no GPU to select) — hardware-independent.
    let Some((model_path, _)) =
        common::smoke_fixtures("gpu_device_with_cpu_backend_is_invalid_argument")
    else {
        return;
    };
    let err = Model::load_with(
        &model_path,
        &ModelOptions {
            backend: Backend::Cpu,
            gpu_device: 1,
        },
    )
    .unwrap_err();
    assert!(matches!(err, Error::InvalidArgument(_)), "got {err:?}");
}
