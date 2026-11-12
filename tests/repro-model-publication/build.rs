use quote::{quote, ToTokens};
use std::{env, fs, path::PathBuf};
use syn::{ImplItem, Item, Type};

const MANAGER: &str = "managers/transcription.rs";
const SUPERVISOR: &str = "engine_supervisor/supervisor.rs";

fn main() {
    let manager = extract(
        MANAGER,
        "TranscriptionManager",
        &[
            "pub fn load_model_with_device(&self, model_id: &str, device_index: Option<usize>,) -> Result<()>",
            "fn begin_unload(&self) -> Unloading",
            "pub fn request_unload(&self)",
            "fn forget_model_if_engine_dropped(&self, model_id: &str, error: &str)",
            "pub fn get_current_model(&self) -> Option<String>",
            "pub fn is_model_loaded(&self) -> bool",
            "fn now_ms() -> u64",
            "fn touch_activity(&self)",
        ],
    );
    // The residency rules the manager relies on: what `loaded` reads after an
    // unload is requested, and when a finished load may publish.
    let supervisor = extract(
        SUPERVISOR,
        "EngineSupervisor",
        &[
            "fn ask<T>(&self, command: impl FnOnce(Reply<T>) -> Command) -> Option<T>",
            "pub fn load(&self, spec: LoadSpec) -> Result<LoadedInfo, EngineError>",
            "pub fn unload(&self) -> Unloading",
            "pub fn loaded(&self) -> Option<LoadedInfo>",
        ],
    );
    let owner = extract(
        SUPERVISOR,
        "Owner",
        &["fn publish_loaded(&self, info: &LoadedInfo)"],
    );
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(out.join("manager_methods.rs"), manager.0).unwrap();
    fs::write(
        out.join("engine_methods.rs"),
        format!("{}\n{}", supervisor.0, owner.0),
    )
    .unwrap();
    println!(
        "cargo:warning=Verified {} complete production methods; no method tokens changed",
        manager.1 + supervisor.1 + owner.1
    );
}

fn parse(relative: &str) -> syn::File {
    let source = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../src-tauri/src")
        .join(relative);
    println!("cargo:rerun-if-changed={}", source.display());
    syn::parse_file(&fs::read_to_string(&source).unwrap()).unwrap()
}

/// The inherent methods of `type_name` with exactly these signatures, as one
/// `impl` block, and how many there are.
fn extract(relative: &str, type_name: &str, signatures: &[&str]) -> (String, usize) {
    let file = parse(relative);
    let methods: Vec<_> = signatures
        .iter()
        .map(|signature| {
            let expected: syn::ImplItemFn = syn::parse_str(&format!("{signature} {{}} ")).unwrap();
            let matches: Vec<_> = file
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::Impl(item) if item.trait_.is_none() => Some(item),
                    _ => None,
                })
                .filter(|item| match item.self_ty.as_ref() {
                    Type::Path(path) => path.path.is_ident(type_name),
                    _ => false,
                })
                .flat_map(|item| &item.items)
                .filter_map(|item| match item {
                    ImplItem::Fn(method) if method.sig.ident == expected.sig.ident => Some(method),
                    _ => None,
                })
                .collect();
            assert_eq!(
                matches.len(),
                1,
                "expected one {type_name}::{}",
                expected.sig.ident
            );
            let method = matches[0];
            assert_eq!(
                method.vis.to_token_stream().to_string(),
                expected.vis.to_token_stream().to_string(),
                "source visibility changed"
            );
            assert_eq!(
                method.sig.to_token_stream().to_string(),
                expected.sig.to_token_stream().to_string(),
                "source signature changed; review extraction"
            );
            method
        })
        .collect();
    let type_name = syn::Ident::new(type_name, proc_macro2::Span::call_site());
    let generated = quote! { impl #type_name { #(#methods)* } };
    let roundtrip: syn::ItemImpl = syn::parse2(generated.clone()).unwrap();
    assert_eq!(roundtrip.items.len(), methods.len());
    for (actual, expected) in roundtrip.items.iter().zip(&methods) {
        assert_eq!(
            actual.to_token_stream().to_string(),
            expected.to_token_stream().to_string(),
            "extracted method changed"
        );
    }
    (generated.to_string(), methods.len())
}
