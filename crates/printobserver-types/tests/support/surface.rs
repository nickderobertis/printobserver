//! Reading a Rust source's declared surface, for the contract tests.
//!
//! These tests hold the port crates to the surface the contract states, and a
//! surface is a property of a declaration rather than of a value, so they read
//! the declarations themselves rather than a table somebody maintains beside
//! them. A fixture trait or a fixture vocabulary is a source snippet read by
//! this same reader, so what refuses a fixture is what reads the committed
//! crate.

use std::path::PathBuf;

use quote::ToTokens;

/// One field of a struct or of a variant's payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// The field's name, or its position for a tuple field.
    pub name: String,
    /// The field's type, with whitespace removed.
    pub ty: String,
}

/// One parameter of a method, excluding the receiver.
pub type Param = Field;

/// One method a trait declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Method {
    /// The method's name.
    pub name: String,
    /// The parameters, excluding the receiver.
    pub params: Vec<Param>,
    /// The return type, with whitespace removed.
    pub returns: String,
}

/// One variant an enum declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    /// The variant's name.
    pub name: String,
    /// The variant's own declared fields.
    pub fields: Vec<Field>,
}

/// A type's whitespace-free spelling, which is what these tests compare.
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

/// The path of one crate of this workspace.
pub fn crate_dir(crate_name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(crate_name)
}

/// One crate's `src/lib.rs`.
///
/// # Panics
///
/// Panics when the crate has no readable `src/lib.rs`.
pub fn crate_source(crate_name: &str) -> String {
    let path = crate_dir(crate_name).join("src").join("lib.rs");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// Every `.rs` source under one crate's `src`, parsed.
///
/// # Panics
///
/// Panics when a source cannot be read or does not parse.
pub fn crate_sources(crate_name: &str) -> Vec<syn::File> {
    let dir = crate_dir(crate_name).join("src");
    let mut sources = Vec::new();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("read {}: {error}", dir.display()))
        .map(|entry| entry.expect("a readable directory entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .collect();
    paths.sort();
    for path in paths {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        sources.push(parse(&text));
    }
    sources
}

/// The fields of one struct or variant declaration.
fn fields_of(fields: &syn::Fields) -> Vec<Field> {
    match fields {
        syn::Fields::Named(named) => named
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
            .collect(),
        syn::Fields::Unnamed(unnamed) => unnamed
            .unnamed
            .iter()
            .enumerate()
            .map(|(index, field)| Field {
                name: index.to_string(),
                ty: spelling(&field.ty),
            })
            .collect(),
        syn::Fields::Unit => Vec::new(),
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
            let syn::TraitItem::Fn(function) = entry else {
                continue;
            };
            let params = function
                .sig
                .inputs
                .iter()
                .filter_map(|input| match input {
                    syn::FnArg::Receiver(_) => None,
                    syn::FnArg::Typed(typed) => Some(Param {
                        name: spelling(&typed.pat),
                        ty: spelling(&typed.ty),
                    }),
                })
                .collect();
            let returns = match &function.sig.output {
                syn::ReturnType::Default => "()".to_owned(),
                syn::ReturnType::Type(_, ty) => spelling(ty),
            };
            methods.push(Method {
                name: function.sig.ident.to_string(),
                params,
                returns,
            });
        }
    }
    methods
}

/// The variants one enum declares, in declaration order.
pub fn enum_variants(file: &syn::File, enum_name: &str) -> Vec<Variant> {
    let mut variants = Vec::new();
    for item in &file.items {
        let syn::Item::Enum(declaration) = item else {
            continue;
        };
        if declaration.ident != enum_name {
            continue;
        }
        for variant in &declaration.variants {
            variants.push(Variant {
                name: variant.ident.to_string(),
                fields: fields_of(&variant.fields),
            });
        }
    }
    variants
}

/// The fields one struct declares.
pub fn struct_fields(file: &syn::File, struct_name: &str) -> Vec<Field> {
    for item in &file.items {
        let syn::Item::Struct(declaration) = item else {
            continue;
        };
        if declaration.ident == struct_name {
            return fields_of(&declaration.fields);
        }
    }
    Vec::new()
}

/// Whether an item is publicly visible.
fn is_public(visibility: &syn::Visibility) -> bool {
    matches!(visibility, syn::Visibility::Public(_))
}

/// Every public struct and enum one source declares, by name.
pub fn declared_type_names(file: &syn::File) -> Vec<String> {
    let mut names = Vec::new();
    for item in &file.items {
        match item {
            syn::Item::Struct(declaration) if is_public(&declaration.vis) => {
                names.push(declaration.ident.to_string());
            }
            syn::Item::Enum(declaration) if is_public(&declaration.vis) => {
                names.push(declaration.ident.to_string());
            }
            _ => {}
        }
    }
    names
}

/// Every public constant one source exports: its name and its type.
pub fn public_constants(file: &syn::File) -> Vec<Field> {
    file.items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Const(declaration) if is_public(&declaration.vis) => Some(Field {
                name: declaration.ident.to_string(),
                ty: spelling(&declaration.ty),
            }),
            _ => None,
        })
        .collect()
}

/// Every public free function one source exports.
pub fn public_functions(file: &syn::File) -> Vec<Method> {
    file.items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Fn(declaration) if is_public(&declaration.vis) => {
                let params = declaration
                    .sig
                    .inputs
                    .iter()
                    .filter_map(|input| match input {
                        syn::FnArg::Receiver(_) => None,
                        syn::FnArg::Typed(typed) => Some(Param {
                            name: spelling(&typed.pat),
                            ty: spelling(&typed.ty),
                        }),
                    })
                    .collect();
                let returns = match &declaration.sig.output {
                    syn::ReturnType::Default => "()".to_owned(),
                    syn::ReturnType::Type(_, ty) => spelling(ty),
                };
                Some(Method {
                    name: declaration.sig.ident.to_string(),
                    params,
                    returns,
                })
            }
            _ => None,
        })
        .collect()
}

/// The documentation attached to one method of one trait.
pub fn trait_method_docs(file: &syn::File, trait_name: &str, method_name: &str) -> String {
    let mut lines = Vec::new();
    for item in &file.items {
        let syn::Item::Trait(declaration) = item else {
            continue;
        };
        if declaration.ident != trait_name {
            continue;
        }
        for entry in &declaration.items {
            let syn::TraitItem::Fn(function) = entry else {
                continue;
            };
            if function.sig.ident != method_name {
                continue;
            }
            for attribute in &function.attrs {
                let syn::Meta::NameValue(pair) = &attribute.meta else {
                    continue;
                };
                if !pair.path.is_ident("doc") {
                    continue;
                }
                let syn::Expr::Lit(literal) = &pair.value else {
                    continue;
                };
                let syn::Lit::Str(text) = &literal.lit else {
                    continue;
                };
                lines.push(text.value());
            }
        }
    }
    lines.join("\n")
}

/// Whether a declaration derives one named trait.
fn derives(attrs: &[syn::Attribute], wanted: &str) -> bool {
    attrs.iter().any(|attribute| {
        attribute.path().is_ident("derive")
            && attribute.to_token_stream().to_string().contains(wanted)
    })
}

/// Every type one source declares that can emit a JSON Schema.
///
/// Three shapes reach that: a declaration deriving `JsonSchema`, one carrying a
/// hand-written `impl JsonSchema`, and an identifier newtype the `identifier!`
/// macro declares, whose expansion no source-level reader can see.
pub fn schema_emitting_types(file: &syn::File) -> Vec<String> {
    let mut names = Vec::new();
    for item in &file.items {
        match item {
            syn::Item::Struct(declaration)
                if is_public(&declaration.vis) && derives(&declaration.attrs, "JsonSchema") =>
            {
                names.push(declaration.ident.to_string());
            }
            syn::Item::Enum(declaration)
                if is_public(&declaration.vis) && derives(&declaration.attrs, "JsonSchema") =>
            {
                names.push(declaration.ident.to_string());
            }
            syn::Item::Impl(block) => {
                let Some((path, _)) = &block.trait_ else {
                    continue;
                };
                if path
                    .segments
                    .last()
                    .is_some_and(|segment| segment.ident == "JsonSchema")
                {
                    names.push(spelling(&block.self_ty).replace("<f64>", ""));
                }
            }
            syn::Item::Macro(invocation) if invocation.mac.path.is_ident("identifier") => {
                let tokens = invocation.mac.tokens.to_string();
                if let Some((name, _)) = tokens.split_once(',') {
                    names.push(name.trim().to_owned());
                }
            }
            _ => {}
        }
    }
    names.sort();
    names.dedup();
    names
}
