#[path = "../method_selector.rs"]
mod method_selector;

use method_selector::method;

fn select(source: &str) -> syn::ImplItemFn {
    method(
        &syn::parse_file(source).expect("parse selector fixture"),
        "AudioRecordingManager",
        "start",
    )
}

#[test]
fn selects_nongeneric_owner() {
    let selected = select("impl AudioRecordingManager { fn start() {} }");
    assert_eq!(selected.sig.ident, "start");
}

#[test]
fn selects_generic_owner() {
    let selected = select("impl<R> AudioRecordingManager<R> { fn start() {} }");
    assert_eq!(selected.sig.ident, "start");
}

#[test]
fn selects_trait_implementation() {
    let selected = select("impl<R> Capture for AudioRecordingManager<R> { fn start() {} }");
    assert_eq!(selected.sig.ident, "start");
}

#[test]
#[should_panic(expected = "AudioRecordingManager::start missing or ambiguous")]
fn rejects_qualified_self() {
    select("impl <Manager>::AudioRecordingManager { fn start() {} }");
}

#[test]
#[should_panic(expected = "AudioRecordingManager::start missing or ambiguous")]
fn rejects_leading_colon() {
    select("impl ::AudioRecordingManager { fn start() {} }");
}

#[test]
#[should_panic(expected = "AudioRecordingManager::start missing or ambiguous")]
fn rejects_multiple_segments() {
    select("impl manager::AudioRecordingManager<R> { fn start() {} }");
}

#[test]
#[should_panic(expected = "AudioRecordingManager::start missing or ambiguous")]
fn rejects_other_owner() {
    select("impl<R> OtherManager<R> { fn start() {} }");
}

#[test]
#[should_panic(expected = "AudioRecordingManager::start missing or ambiguous")]
fn rejects_missing_method() {
    select("impl<R> AudioRecordingManager<R> { fn stop() {} }");
}

#[test]
#[should_panic(expected = "AudioRecordingManager::start missing or ambiguous")]
fn rejects_ambiguous_method() {
    select(
        "impl AudioRecordingManager { fn start() {} }
         impl<R> Capture for AudioRecordingManager<R> { fn start() {} }",
    );
}
