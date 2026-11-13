//! Lists what a block and an item builder can be told, from the SDK's source: every public method
//! taking the builder and giving it back, its parameters as written and its first doc line. The
//! editor builds its form from this, so a method added to the SDK shows up there with no edit.

use std::fmt::Write as _;
use std::path::Path;

use quote::ToTokens;
use syn::{FnArg, ImplItem, Item, Pat, ReturnType, TraitItem, Type};

/// Which builders a block of methods is on.
const BLOCK: u8 = 1;
const ITEM: u8 = 2;

struct Method {
    on: u8,
    name: String,
    /// (name, type, the folder of the picture it names: what `asset(folder, ..)` it reaches, or the
    /// builder's own for a texture setter; empty if it names none).
    params: Vec<(String, String, String)>,
    doc: String,
}

/// The first paragraph of its doc comment; `None` when `#[doc(hidden)]`.
fn doc_of(attrs: &[syn::Attribute]) -> Option<String> {
    let mut lines = Vec::new();
    let mut ended = false;
    for a in attrs.iter().filter(|a| a.path().is_ident("doc")) {
        match &a.meta {
            syn::Meta::List(l) if l.tokens.to_string().contains("hidden") => return None,
            syn::Meta::NameValue(nv) if !ended => {
                if let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) = &nv.value {
                    let l = s.value().trim().to_string();
                    ended = l.is_empty();
                    if !ended {
                        lines.push(l);
                    }
                }
            }
            _ => {}
        }
    }
    Some(lines.join(" "))
}

fn ty(t: &Type) -> String {
    t.to_token_stream()
        .to_string()
        .replace(" ", "")
        .replace("'_", "")
        .replace("'a", "")
        .replace("&mut", "&mut ")
}

fn method(on: u8, sig: &syn::Signature, attrs: &[syn::Attribute], body: String, public: bool, out: &mut Vec<Method>) {
    let Some(doc) = doc_of(attrs) else { return };
    let takes_self = matches!(sig.inputs.first(), Some(FnArg::Receiver(r)) if r.reference.is_none());
    let gives_self = matches!(&sig.output, ReturnType::Type(_, t) if t.to_token_stream().to_string() == "Self");
    if !public || !takes_self || !gives_self || !sig.generics.params.is_empty() {
        return;
    }
    let body = body.replace(' ', "");
    let own = if on == ITEM { "item" } else { "block" };
    let params = sig
        .inputs
        .iter()
        .skip(1)
        .filter_map(|a| match a {
            FnArg::Typed(p) => {
                let name = match &*p.pat {
                    Pat::Ident(i) => i.ident.to_string(),
                    _ => "_".into(),
                };
                let t = ty(&p.ty);
                let folder = if t != "&str" {
                    String::new()
                } else if let Some(f) = body.split("asset(\"").skip(1).find(|rest| {
                    rest.split_once(',')
                        .is_some_and(|(_, a)| a.starts_with(&name))
                }) {
                    f.split('"').next().unwrap_or("").to_string()
                } else if [".pick(", ".texture(", ".layer("]
                    .iter()
                    .any(|k| body.contains(&format!("{k}{name}")) || body.contains(&format!(",{name})")) && body.contains(k))
                {
                    own.to_string()
                } else {
                    String::new()
                };
                Some((name, t, folder))
            }
            _ => None,
        })
        .collect();
    out.push(Method { on, name: sig.ident.to_string(), params, doc });
}

fn impl_items(on: u8, items: &[ImplItem], out: &mut Vec<Method>) {
    for it in items {
        if let ImplItem::Fn(f) = it {
            let public = matches!(f.vis, syn::Visibility::Public(_));
            method(on, &f.sig, &f.attrs, f.block.to_token_stream().to_string(), public, out);
        }
    }
}

fn scan(file: &syn::File, out: &mut Vec<Method>) {
    for item in &file.items {
        match item {
            Item::Impl(i) if i.trait_.is_none() => {
                let target = i.self_ty.to_token_stream().to_string();
                let on = if target.starts_with("BlockBuilder") {
                    BLOCK
                } else if target.starts_with("ItemBuilder") {
                    ITEM
                } else {
                    continue;
                };
                impl_items(on, &i.items, out);
            }
            // Traits whose methods the builders get: what every component holder has, and what
            // items have beside entities.
            Item::Trait(t) => {
                let on = match t.ident.to_string().as_str() {
                    "WithComponents" => BLOCK | ITEM,
                    "Holds" | "Glows" => ITEM,
                    _ => continue,
                };
                for ti in &t.items {
                    if let TraitItem::Fn(f) = ti
                        && let Some(b) = &f.default
                    {
                        method(on, &f.sig, &f.attrs, b.to_token_stream().to_string(), true, out);
                    }
                }
            }
            // The setters a block shares with its states are written once in a macro.
            Item::Macro(m) if m.mac.path.is_ident("state_setters") || m.ident.as_ref().is_some_and(|i| i == "state_setters") => {
                let tokens: Vec<proc_macro2::TokenTree> = m.mac.tokens.clone().into_iter().collect();
                let Some(proc_macro2::TokenTree::Group(body)) = tokens
                    .iter()
                    .rev()
                    .find(|t| matches!(t, proc_macro2::TokenTree::Group(_)))
                else {
                    continue;
                };
                let wrapped: syn::Result<syn::ItemImpl> = syn::parse2(quote::quote!(impl X { #body }));
                let inner = match wrapped {
                    Ok(i) => i,
                    Err(_) => match syn::parse2::<syn::ItemImpl>(quote::quote!(impl X #body)) {
                        Ok(i) => i,
                        Err(_) => continue,
                    },
                };
                impl_items(BLOCK, &inner.items, out);
            }
            Item::Mod(m) => {
                if let Some((_, items)) = &m.content {
                    scan(&syn::File { shebang: None, attrs: Vec::new(), items: items.clone() }, out);
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let sdk = Path::new(env!("CARGO_MANIFEST_DIR")).join("../sdk/src");
    println!("cargo:rerun-if-changed={}", sdk.display());
    let mut methods = Vec::new();
    let mut files: Vec<_> = std::fs::read_dir(&sdk)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    files.sort();
    for path in files
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
    {
        let src = std::fs::read_to_string(path).unwrap();
        // A file that doesn't parse is the compiler's to report, not this list's.
        if let Ok(file) = syn::parse_file(&src) {
            scan(&file, &mut methods);
        }
    }
    // One entry per name and builder: a method in several impls (`light` on a block and its
    // states) is the same call.
    methods.sort_by(|a, b| a.name.cmp(&b.name));
    methods.dedup_by(|a, b| a.name == b.name && a.on == b.on);
    let mut out = String::from("pub static METHODS: &[Method] = &[\n");
    for m in &methods {
        let params: Vec<String> = m
            .params
            .iter()
            .map(|(n, t, f)| format!("Param {{ name: {n:?}, ty: {t:?}, picture: {f:?} }}"))
            .collect();
        let _ = writeln!(out, "    Method {{ on: {}, name: {:?}, params: &[{}], doc: {:?} }},", m.on, m.name, params.join(", "), m.doc);
    }
    out += "];\n";
    std::fs::write(Path::new(&std::env::var("OUT_DIR").unwrap()).join("sdk_methods.rs"), out).unwrap();
}
