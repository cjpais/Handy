use quote::{format_ident, quote, ToTokens};
use std::{env, fs, path::PathBuf};
use syn::{ImplItem, ImplItemFn, Item, Stmt, Type};

fn method<'a>(file: &'a syn::File, owner: &str, name: &str) -> &'a ImplItemFn {
    let found: Vec<_> = file
        .items
        .iter()
        .filter_map(|item| {
            let Item::Impl(item) = item else { return None };
            let Type::Path(path) = item.self_ty.as_ref() else {
                return None;
            };
            path.path
                .segments
                .last()
                .filter(|segment| segment.ident == owner)
                .map(|_| item)
        })
        .flat_map(|item| &item.items)
        .filter_map(|item| match item {
            ImplItem::Fn(item) if item.sig.ident == name => Some(item),
            _ => None,
        })
        .collect();
    assert_eq!(found.len(), 1, "{owner}::{name} missing or ambiguous");
    found[0]
}

fn methods(file: &syn::File, owner: &str, names: &[&str]) -> proc_macro2::TokenStream {
    let items: Vec<_> = names.iter().map(|name| method(file, owner, name)).collect();
    let owner = format_ident!("{owner}");
    quote! { impl #owner { #(#items)* } }
}

fn named(file: &syn::File, name: &str) -> proc_macro2::TokenStream {
    let found: Vec<_> = file
        .items
        .iter()
        .filter(|item| match item {
            Item::Struct(item) => item.ident == name,
            Item::Enum(item) => item.ident == name,
            _ => false,
        })
        .collect();
    assert_eq!(found.len(), 1, "{name} missing or ambiguous");
    let item = found[0];
    quote! { #item }
}

fn compact(item: impl ToTokens) -> String {
    item.to_token_stream()
        .to_string()
        .split_whitespace()
        .collect()
}

fn ordered(body: &str, operations: &[&str]) {
    let mut remaining = body;
    for operation in operations {
        let offset = remaining.find(operation).unwrap_or_else(|| {
            panic!("source correspondence drift at {operation}; review replay assumptions")
        });
        remaining = &remaining[offset + operation.len()..];
    }
}

fn parse(path: PathBuf) -> syn::File {
    println!("cargo:rerun-if-changed={}", path.display());
    syn::parse_file(&fs::read_to_string(path).expect("read source")).expect("parse source")
}

fn recorder_boundary() {
    println!("cargo:rerun-if-env-changed=HANDY_RECORDER_SOURCE");
    let Some(path) = env::var_os("HANDY_RECORDER_SOURCE") else {
        println!("cargo:warning=recorder timeout/detach gate skipped; set HANDY_RECORDER_SOURCE to handy-recorder 0.1.0 source");
        return;
    };
    let path = PathBuf::from(path);
    let manifest = fs::read_to_string(path.join("Cargo.toml")).expect("recorder manifest");
    assert!(manifest.contains("name = \"handy-recorder\""));
    assert!(manifest.contains("version = \"0.1.0\""));
    let recorder = parse(path.join("src/capture/engine.rs"));
    let stop = compact(method(&recorder, "Engine", "stop"));
    ordered(
        &stop,
        &[
            "letresult=self.collect();",
            "*self.slot.lock().unwrap()=Slot::Idle",
            "result",
        ],
    );
    let collect = compact(method(&recorder, "Engine", "collect"));
    ordered(
        &collect,
        &[
            "reply_rx.recv_timeout(self.timeouts.stop)",
            "Err(mpsc::RecvTimeoutError::Timeout)=>{",
            "ErrorKind::SinkStalled",
            "Err(failure)",
        ],
    );
    assert!(
        !collect.contains(".join("),
        "collect now joins delivery; review boundary"
    );
    let shutdown = compact(method(&recorder, "Engine", "shutdown"));
    ordered(
        &shutdown,
        &[
            ".delivery_exit.recv_timeout(self.timeouts.delivery_exit)",
            "threads.delivery.join()",
            "Err(mpsc::RecvTimeoutError::Timeout)=>{",
            "itisdetached",
        ],
    );
    println!(
        "cargo:warning=handy-recorder 0.1.0 stop timeout and delivery detach source gates passed"
    );
}

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.parent().unwrap().parent().unwrap();
    let manager = parse(root.join("src-tauri/src/managers/transcription.rs"));
    let audio = parse(root.join("src-tauri/src/managers/audio.rs"));
    let sink = parse(root.join("src-tauri/src/audio_toolkit/audio/sink.rs"));
    let actions = parse(root.join("src-tauri/src/actions.rs"));
    let lock_path = root.join("src-tauri/Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock_path.display());
    let lock = fs::read_to_string(lock_path).expect("product lockfile");
    assert!(lock.split("[[package]]").any(|package| {
        package.contains("name = \"handy-recorder\"")
            && package.contains("version = \"0.1.0\"")
            && package.contains("checksum = \"4a734d6d16eb8eb485548d75f383fa0c09d65b037a20e648c19730860f7a8a3f\"")
    }), "replay boundary requires locked handy-recorder 0.1.0");

    let constructor = method(&audio, "AudioRecordingManager", "new");
    let callbacks: Vec<_> = constructor
        .block
        .stmts
        .iter()
        .filter_map(|statement| {
            let Stmt::Local(local) = statement else {
                return None;
            };
            let syn::Pat::Type(typed) = &local.pat else {
                return None;
            };
            let syn::Pat::Ident(name) = typed.pat.as_ref() else {
                return None;
            };
            (name.ident == "on_audio").then_some(local)
        })
        .collect();
    assert_eq!(
        callbacks.len(),
        1,
        "manager on_audio declaration missing or ambiguous"
    );
    let initializer = &callbacks[0]
        .init
        .as_ref()
        .expect("on_audio initializer")
        .expr;
    assert_eq!(
        compact(initializer),
        "Arc::new(move|frame|stream_router.feed(frame))",
        "manager callback changed; review frame ownership"
    );
    assert!(
        compact(constructor).contains("on_levels,on_audio,is_open:"),
        "callback must be stored in the manager"
    );
    ordered(
        &compact(method(
            &audio,
            "AudioRecordingManager",
            "try_start_recording",
        )),
        &[
            "letsink=CaptureSink::new(",
            "Arc::clone(&self.on_audio)",
            "microphone.recorder.start(sink)",
        ],
    );
    ordered(
        &compact(method(&actions, "TranscribeAction", "start")),
        &["tm.start_stream()", "rm.try_start_recording("],
    );
    ordered(
        &compact(method(&audio, "AudioRecordingManager", "collect_recording")),
        &[
            "microphone.recorder.stop()",
            "Err(e)=>{",
            "microphone.failed.store(true,Ordering::Release)",
            "Vec::new()",
        ],
    );
    ordered(
        &compact(method(&audio, "AudioRecordingManager", "stop_recording")),
        &[
            "self.collect_recording(microphone)",
            "*self.is_recording.lock().unwrap()=false",
            "RecordingState::Idle",
        ],
    );
    ordered(
        &compact(method(
            &audio,
            "AudioRecordingManager",
            "start_microphone_stream",
        )),
        &[
            "OpenMicrophone::needs_reopen",
            "self.close_microphone(microphone)",
            "open_microphone_on_channel(",
        ],
    );
    ordered(
        &compact(method(&audio, "AudioRecordingManager", "close_microphone")),
        &["microphone.recorder.close()", "warn!"],
    );
    ordered(
        &compact(method(&actions, "TranscribeAction", "stop")),
        &["samples.is_empty()", "tm.cancel_stream()"],
    );
    recorder_boundary();

    let mut output = proc_macro2::TokenStream::new();
    for name in ["StreamRouter", "Route", "StreamCmd"] {
        output.extend(named(&manager, name));
    }
    output.extend(methods(
        &manager,
        "StreamRouter",
        &["new", "open", "take", "feed"],
    ));
    output.extend(methods(
        &manager,
        "TranscriptionManager",
        &["cancel_stream"],
    ));
    output.extend(methods(&sink, "CaptureSink", &["emit"]));
    output.extend(quote! {
        fn manager_audio_callback(stream_router: Arc<StreamRouter>) -> AudioFrameCallback {
            #initializer
        }
    });
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("production.rs"),
        output.to_string(),
    )
    .expect("write source extraction");
    println!("cargo:warning=main source gates passed; real router, emit, cancellation and callback extracted");
}
