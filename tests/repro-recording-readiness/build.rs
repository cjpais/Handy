use quote::{quote, ToTokens};
use std::{env, fs, path::Path};
use syn::{visit::Visit, visit_mut::VisitMut, Expr, ImplItem, Item, Type};

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
                && text.contains("rm_clone . is_recording_readiness_current")
            {
                self.0.push(body.block.clone());
            }
        }
        syn::visit::visit_expr_closure(self, closure);
    }
}

struct MuteAdapter;

impl VisitMut for MuteAdapter {
    fn visit_expr_mut(&mut self, expression: &mut Expr) {
        if let Expr::Call(call) = expression {
            if let Expr::Path(path) = call.func.as_ref() {
                if ["get_mute", "set_mute", "restore_mute"]
                    .iter()
                    .any(|name| path.path.is_ident(name))
                {
                    let method = &path.path.segments[0].ident;
                    let arguments = &call.args;
                    *expression = syn::parse_quote! { self.app_handle.#method(#arguments) };
                    return;
                }
            }
        }
        syn::visit_mut::visit_expr_mut(self, expression);
    }
}

fn main() {
    let manifest = std::path::PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.parent().unwrap().parent().unwrap();
    let actions = read(root, "src-tauri/src/actions.rs");
    let audio = read(root, "src-tauri/src/managers/audio.rs");
    let overlay = read(root, "src-tauri/src/overlay.rs");
    let manager = audio
        .items
        .iter()
        .find_map(|item| match item {
            Item::Struct(item) if item.ident == "AudioRecordingManager" => Some(item),
            _ => None,
        })
        .expect("AudioRecordingManager missing");
    for (name, expected) in [
        ("capture_generation", quote! { Arc<AtomicU64> }),
        ("is_open", quote! { Arc<Mutex<bool>> }),
        ("mute_state", quote! { Arc<Mutex<MuteState>> }),
    ] {
        let field = manager
            .fields
            .iter()
            .find(|field| field.ident.as_ref().is_some_and(|ident| ident == name))
            .expect("covered audio field missing");
        let actual = &field.ty;
        let normalize = |tokens: proc_macro2::TokenStream| {
            tokens.to_string().split_whitespace().collect::<String>()
        };
        assert_eq!(
            normalize(quote!(#actual)),
            normalize(expected),
            "audio {name} type changed"
        );
    }
    let mut closures = ReadinessClosure::default();
    closures.visit_file(&actions);
    assert_eq!(
        closures.0.len(),
        1,
        "readiness closure missing or ambiguous"
    );
    let ready = &closures.0[0];
    let mut output = quote! {
        fn production_ready(rm_clone: Arc<AudioRecordingManager>, generation: u64, app_clone: AppHandle) {
            let readiness = Ready;
            #ready
        }
    };

    let mute_state: Vec<_> = audio
        .items
        .iter()
        .filter(|item| matches!(item, Item::Struct(item) if item.ident == "MuteState"))
        .collect();
    assert_eq!(mute_state.len(), 1, "MuteState missing or ambiguous");
    mute_state[0].to_tokens(&mut output);

    let mut methods = Vec::new();
    for name in [
        "invalidate_recording_readiness",
        "is_recording_readiness_current",
        "apply_mute",
        "remove_mute",
    ] {
        let found: Vec<_> = audio.items.iter().filter_map(|item| {
            let Item::Impl(item) = item else { return None };
            matches!(item.self_ty.as_ref(), Type::Path(path) if path.path.is_ident("AudioRecordingManager"))
                .then_some(item)
        }).flat_map(|item| &item.items).filter_map(|item| {
            match item { ImplItem::Fn(item) if item.sig.ident == name => Some(item.clone()), _ => None }
        }).collect();
        assert_eq!(found.len(), 1, "audio {name} missing or ambiguous");
        let mut method = found[0].clone();
        MuteAdapter.visit_impl_item_fn_mut(&mut method);
        methods.push(method);
    }
    output.extend(quote! { impl AudioRecordingManager { #(#methods)* } });
    let emit: Vec<_> = overlay
        .items
        .iter()
        .filter(|item| matches!(item, Item::Fn(item) if item.sig.ident == "emit_recording_ready"))
        .collect();
    assert_eq!(
        emit.len(),
        1,
        "overlay emit_recording_ready missing or ambiguous"
    );
    emit[0].to_tokens(&mut output);
    let file = syn::parse2(output).expect("parse extracted production code");
    fs::write(
        std::path::PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("production.rs"),
        prettyplease::unparse(&file),
    )
    .expect("write extracted production code");
}
