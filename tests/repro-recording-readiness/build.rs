use quote::{quote, ToTokens};
use std::{env, fs, path::Path};
use syn::{visit::Visit, Expr, ImplItem, Item, Type};

fn read(root: &Path, path: &str) -> syn::File {
    let source = root.join(path);
    println!("cargo:rerun-if-changed={}", source.display());
    syn::parse_file(&fs::read_to_string(source).expect("read production Rust"))
        .expect("parse production Rust")
}

#[derive(Default)]
struct ReadinessClosure(Vec<syn::Block>);

impl<'ast> Visit<'ast> for ReadinessClosure {
    fn visit_expr_closure(&mut self, closure: &'ast syn::ExprClosure) {
        if let Expr::Block(body) = closure.body.as_ref() {
            let text = body.to_token_stream().to_string();
            if text.contains("readiness . wait ()")
                && text.contains("play_start_feedback_if_current")
            {
                self.0.push(body.block.clone());
            }
        }
        syn::visit::visit_expr_closure(self, closure);
    }
}

fn method(file: &syn::File, owner: &str, name: &str) -> syn::ImplItemFn {
    let found: Vec<_> = file
        .items
        .iter()
        .filter_map(|item| {
            let Item::Impl(item) = item else { return None };
            matches!(item.self_ty.as_ref(), Type::Path(path) if path.path.is_ident(owner))
                .then_some(item)
        })
        .flat_map(|item| &item.items)
        .filter_map(|item| match item {
            ImplItem::Fn(item) if item.sig.ident == name => Some(item.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(found.len(), 1, "{owner}::{name} missing or ambiguous");
    found[0].clone()
}

fn function(file: &syn::File, name: &str) -> syn::ItemFn {
    let found: Vec<_> = file
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Fn(item) if item.sig.ident == name => Some(item.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(found.len(), 1, "{name} missing or ambiguous");
    found[0].clone()
}

fn assert_order(source: &str, before: &str, after: &str) {
    assert!(
        source.find(before).expect(before) < source.find(after).expect(after),
        "{before} must precede {after}"
    );
}

fn calls(statement: &syn::Stmt, name: &str) -> bool {
    let expression = match statement {
        syn::Stmt::Local(local) => local.init.as_ref().map(|init| init.expr.as_ref()),
        syn::Stmt::Expr(expression, _) => Some(expression),
        _ => None,
    };
    matches!(expression, Some(Expr::MethodCall(call)) if call.method == name)
}

fn admission_prefixes(audio: &syn::File) -> proc_macro2::TokenStream {
    let start = method(audio, "AudioRecordingManager", "try_start_recording");
    let mint = start
        .block
        .stmts
        .iter()
        .position(|stmt| calls(stmt, "begin_recording_readiness"))
        .expect("startup ownership allocation missing");
    let start = &start.block.stmts[..=mint];
    let cancel = method(audio, "AudioRecordingManager", "cancel_recording");
    let dispatch = cancel
        .block
        .stmts
        .iter()
        .position(|stmt| matches!(stmt, syn::Stmt::Expr(Expr::Match(_), _)))
        .expect("cancel state dispatch missing");
    let cancel = &cancel.block.stmts[..dispatch];
    let stop = method(audio, "AudioRecordingManager", "stop_recording");
    let dispatch = stop
        .block
        .stmts
        .iter()
        .position(|stmt| matches!(stmt, syn::Stmt::Expr(Expr::Match(_), _)))
        .expect("stop state dispatch missing");
    let prefix = &stop.block.stmts[..dispatch];
    let syn::Stmt::Expr(Expr::Match(mut dispatch), _) = stop.block.stmts[dispatch].clone() else {
        unreachable!()
    };
    let Expr::Block(body) = dispatch.arms[0].body.as_mut() else {
        panic!("stop arm must be a block")
    };
    let transition = body
        .block
        .stmts
        .iter()
        .position(|stmt| calls(stmt, "set_state"))
        .expect("stop state transition missing");
    body.block.stmts.truncate(transition);
    body.block.stmts.push(syn::Stmt::Expr(
        syn::parse_quote! { Some(observe(&mut state)) },
        None,
    ));
    quote! {
        impl AudioRecordingManager {
            fn production_start_admission<R>(&self, complete: impl FnOnce(RecordingReadinessOwnership, &mut RecordingState) -> R) -> Result<R, String> {
                #(#start)*
                Ok(complete(ownership, &mut state))
            }
            fn production_cancel_admission<R>(&self, observe: impl FnOnce(&mut RecordingState) -> R) -> R {
                #(#cancel)*
                observe(&mut state)
            }
            fn production_stop_admission<R>(&self, binding_id: &str, observe: impl FnOnce(&mut RecordingState) -> R) -> Option<R> {
                #(#prefix)*
                #dispatch
            }
        }
    }
}

fn main() {
    let manifest = std::path::PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.parent().unwrap().parent().unwrap();
    let actions = read(root, "src-tauri/src/actions.rs");
    let audio = read(root, "src-tauri/src/managers/audio.rs");
    let overlay = read(root, "src-tauri/src/overlay.rs");
    let feedback = read(root, "src-tauri/src/audio_feedback.rs");
    let manager = audio
        .items
        .iter()
        .find_map(|item| match item {
            Item::Struct(item) if item.ident == "AudioRecordingManager" => Some(item),
            _ => None,
        })
        .expect("AudioRecordingManager missing");
    let field = manager
        .fields
        .iter()
        .find(|field| {
            field
                .ident
                .as_ref()
                .is_some_and(|ident| ident == "capture_generation")
        })
        .expect("capture_generation missing");
    let actual = &field.ty;
    let normalize = |tokens: proc_macro2::TokenStream| {
        tokens.to_string().split_whitespace().collect::<String>()
    };
    assert_eq!(
        normalize(quote!(#actual)),
        normalize(quote!(Arc<Mutex<u64>>))
    );

    let start = method(&audio, "AudioRecordingManager", "try_start_recording")
        .to_token_stream()
        .to_string();
    assert_order(
        &start,
        "RecordingState :: Idle",
        "self . begin_recording_readiness ()",
    );
    assert_order(
        &start,
        "self . begin_recording_readiness ()",
        "self . start_microphone_stream ()",
    );
    assert_eq!(start.matches("begin_recording_readiness").count(), 1);
    assert!(
        !start.contains("capture_generation"),
        "startup must not renew ownership after opening"
    );
    assert!(start.contains("RecordingReadiness { receiver , ownership , }"));
    let action_start = method(&actions, "TranscribeAction", "start")
        .to_token_stream()
        .to_string();
    assert_order(
        &action_start,
        "rm . invalidate_recording_readiness ()",
        "show_streaming_overlay",
    );
    assert_order(
        &action_start,
        "rm . invalidate_recording_readiness ()",
        "show_recording_overlay",
    );
    assert_order(
        &action_start,
        "show_recording_overlay",
        "rm . try_start_recording",
    );
    for name in ["stop_recording", "cancel_recording"] {
        let body = method(&audio, "AudioRecordingManager", name).block;
        assert!(body.stmts[0]
            .to_token_stream()
            .to_string()
            .contains("invalidate_recording_readiness"));
    }
    let stop = method(&actions, "TranscribeAction", "stop").block;
    assert!(stop.stmts[0]
        .to_token_stream()
        .to_string()
        .contains("invalidate_recording_readiness"));

    let mut closures = ReadinessClosure::default();
    closures.visit_file(&actions);
    assert_eq!(
        closures.0.len(),
        1,
        "readiness closure missing or ambiguous"
    );
    let ready = &closures.0[0];
    let mut output = quote! {
        fn production_ready(rm_clone: Arc<AudioRecordingManager>, readiness: RecordingReadiness, app_clone: AppHandle) {
            let generation = readiness.generation();
            let ownership = readiness.ownership();
            #ready
        }
    };
    output.extend(admission_prefixes(&audio));
    let states: Vec<_> = audio
        .items
        .iter()
        .filter(|item| matches!(item, Item::Enum(item) if item.ident == "RecordingState"))
        .collect();
    assert_eq!(states.len(), 1, "RecordingState missing or ambiguous");
    states[0].to_tokens(&mut output);
    for name in ["RecordingReadiness", "RecordingReadinessOwnership"] {
        let structures: Vec<_> = audio
            .items
            .iter()
            .filter(|item| matches!(item, Item::Struct(item) if item.ident == name))
            .collect();
        assert_eq!(structures.len(), 1);
        structures[0].to_tokens(&mut output);
        let implementations: Vec<_> = audio.items.iter().filter(|item|
            matches!(item, Item::Impl(item) if matches!(item.self_ty.as_ref(), Type::Path(path) if path.path.is_ident(name)))).collect();
        assert_eq!(implementations.len(), 1);
        implementations[0].to_tokens(&mut output);
    }
    let methods: Vec<_> = [
        "begin_recording_readiness",
        "invalidate_recording_readiness",
        "is_recording_readiness_current",
    ]
    .iter()
    .map(|name| method(&audio, "AudioRecordingManager", name))
    .collect();
    output.extend(quote! { impl AudioRecordingManager { #(#methods)* } });
    function(&overlay, "emit_recording_ready").to_tokens(&mut output);
    for name in ["play_start_feedback_if_current", "play_audio_file"] {
        function(&feedback, name).to_tokens(&mut output);
    }
    let file = syn::parse2(output).expect("parse extracted production code");
    fs::write(
        std::path::PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("production.rs"),
        prettyplease::unparse(&file),
    )
    .expect("write extracted production code");
}
