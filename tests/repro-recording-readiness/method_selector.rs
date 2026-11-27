use syn::{ImplItem, Item, Type};

pub(crate) fn method(file: &syn::File, owner: &str, name: &str) -> syn::ImplItemFn {
    let found: Vec<_> = file
        .items
        .iter()
        .filter_map(|item| {
            let Item::Impl(item) = item else { return None };
            matches!(item.self_ty.as_ref(), Type::Path(path)
                if path.qself.is_none()
                    && path.path.leading_colon.is_none()
                    && path.path.segments.len() == 1
                    && path.path.segments[0].ident == owner)
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
