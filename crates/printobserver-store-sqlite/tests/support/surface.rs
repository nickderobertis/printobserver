//! Reading a Rust source's declared surface, for the checks that derive from it.
//!
//! Three of this crate's checks are about declarations rather than about
//! values: which record kinds the contracts declare, which references those
//! records make, and whether any method answers a collection of events under no
//! bound. Each of them reads the declarations themselves rather than a table
//! maintained beside them, and every fixture they are driven against is a source
//! snippet read by this same reader — so what refuses a fixture is what reads
//! the committed crate.

#![allow(dead_code)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use quote::ToTokens;

/// One field of a struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// The field's name.
    pub name: String,
    /// The field's type, with whitespace removed.
    pub ty: String,
}

/// One method a trait or an implementation declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Method {
    /// The type or trait the method belongs to.
    pub owner: String,
    /// The method's name.
    pub name: String,
    /// The parameters, excluding the receiver.
    pub params: Vec<Field>,
    /// The return type, with whitespace removed.
    pub returns: String,
}

/// A node's whitespace-free spelling, which is what these checks compare.
fn spelling<T: ToTokens>(node: &T) -> String {
    node.to_token_stream().to_string().replace([' ', '\n'], "")
}

/// Parse one Rust source.
///
/// # Panics
///
/// Panics when the source does not parse, which is what a broken fixture is.
pub fn parse(source: &str) -> syn::File {
    syn::parse_file(source).expect("the source parses as Rust")
}

/// The directory of one crate of this workspace.
pub fn crate_dir(crate_name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(crate_name)
}

/// One file's text.
///
/// # Panics
///
/// Panics when the file cannot be read, which is a broken checkout.
pub fn read(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// Every `.rs` source under one crate's `src`, parsed, in path order.
///
/// # Panics
///
/// Panics when a source cannot be read or does not parse.
pub fn crate_sources(crate_name: &str) -> Vec<syn::File> {
    let dir = crate_dir(crate_name).join("src");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("read {}: {error}", dir.display()))
        .map(|entry| entry.expect("a readable directory entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .collect();
    paths.sort();
    paths.iter().map(|path| parse(&read(path))).collect()
}

/// The parameters and answer of one function signature.
fn signature(owner: &str, function: &syn::Signature) -> Method {
    let params = function
        .inputs
        .iter()
        .filter_map(|input| match input {
            syn::FnArg::Receiver(_) => None,
            syn::FnArg::Typed(typed) => Some(Field {
                name: spelling(&typed.pat),
                ty: spelling(&typed.ty),
            }),
        })
        .collect();
    let returns = match &function.output {
        syn::ReturnType::Default => "()".to_owned(),
        syn::ReturnType::Type(_, ty) => spelling(ty),
    };
    Method {
        owner: owner.to_owned(),
        name: function.ident.to_string(),
        params,
        returns,
    }
}

/// The methods one trait declares, in declaration order.
pub fn trait_methods(file: &syn::File, trait_name: &str) -> Vec<Method> {
    let mut methods = Vec::new();
    for item in &file.items {
        let syn::Item::Trait(declaration) = item else {
            continue;
        };
        if declaration.ident != trait_name {
            continue;
        }
        for entry in &declaration.items {
            if let syn::TraitItem::Fn(function) = entry {
                methods.push(signature(trait_name, &function.sig));
            }
        }
    }
    methods
}

/// Every method one type exposes to a caller, across every `impl` block.
///
/// An inherent method is exposed when it is public; a method of a trait
/// implementation is exposed by the trait, whatever its own spelling says.
pub fn exposed_methods(files: &[syn::File], type_name: &str) -> Vec<Method> {
    let mut methods = Vec::new();
    for file in files {
        for item in &file.items {
            let syn::Item::Impl(block) = item else {
                continue;
            };
            if spelling(&block.self_ty) != type_name {
                continue;
            }
            let is_trait_impl = block.trait_.is_some();
            for entry in &block.items {
                let syn::ImplItem::Fn(function) = entry else {
                    continue;
                };
                let public = matches!(function.vis, syn::Visibility::Public(_));
                if is_trait_impl || public {
                    methods.push(signature(type_name, &function.sig));
                }
            }
        }
    }
    methods
}

/// The fields each named struct declares, across a crate's sources.
pub fn struct_fields(files: &[syn::File], struct_name: &str) -> Vec<Field> {
    for file in files {
        for item in &file.items {
            let syn::Item::Struct(declaration) = item else {
                continue;
            };
            if declaration.ident != struct_name {
                continue;
            }
            if let syn::Fields::Named(named) = &declaration.fields {
                return named
                    .named
                    .iter()
                    .map(|field| Field {
                        name: field
                            .ident
                            .as_ref()
                            .expect("a named field has a name")
                            .to_string(),
                        ty: spelling(&field.ty),
                    })
                    .collect();
            }
        }
    }
    Vec::new()
}

/// Every public struct one crate's sources declare, by name.
pub fn struct_names(files: &[syn::File]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for file in files {
        for item in &file.items {
            if let syn::Item::Struct(declaration) = item
                && matches!(declaration.vis, syn::Visibility::Public(_))
            {
                names.insert(declaration.ident.to_string());
            }
        }
    }
    names
}

/// Every method name one source calls on anything.
///
/// Read off the parsed tokens rather than the text, so that a name inside a
/// comment or a string literal is not a call.
pub fn method_calls(file: &syn::File) -> BTreeSet<String> {
    let tokens = file.to_token_stream().to_string();
    let mut names = BTreeSet::new();
    let bytes: Vec<&str> = tokens.split_whitespace().collect();
    for window in bytes.windows(3) {
        if window[0] == "." && window[2].starts_with('(') {
            names.insert(window[1].to_owned());
        }
    }
    names
}

/// Every identifier a type's spelling names.
pub fn named_types(spelling: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut current = String::new();
    for character in spelling.chars() {
        if character.is_alphanumeric() || character == '_' {
            current.push(character);
        } else if !current.is_empty() {
            names.insert(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        names.insert(current);
    }
    names
}
