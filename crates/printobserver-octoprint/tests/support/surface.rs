//! Reading a Rust source's declared surface, for the checks that derive from it.
//!
//! Three of this crate's claims are about coverage of a contract this crate does
//! not own: every method the printer port declares is driven, every variant of
//! its error vocabulary is produced, and every field the two snapshot contracts
//! declare is walked. Each of those is checked by reading the declarations
//! themselves and the journey's own calls, rather than a list maintained beside
//! either — so a contract that grows fails the check rather than quietly
//! outgrowing the coverage.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use quote::ToTokens as _;

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
pub fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// Parse one Rust source.
///
/// # Panics
///
/// Panics when the source does not parse, which is what a broken fixture is.
pub fn parse(source: &str) -> syn::File {
    syn::parse_file(source).expect("the source parses as Rust")
}

/// One of this crate's own test sources, parsed.
///
/// # Panics
///
/// Panics when the source cannot be read or does not parse.
pub fn test_source(relative: &str) -> syn::File {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(relative);
    parse(&read(&path))
}

/// The printer port, as this repository's port crate declares it.
///
/// # Panics
///
/// Panics when the port crate cannot be read or does not parse.
pub fn port() -> syn::File {
    parse(&read(
        &crate_dir("printobserver-printer-api")
            .join("src")
            .join("lib.rs"),
    ))
}

/// One source of the type crate, parsed.
///
/// # Panics
///
/// Panics when the source cannot be read or does not parse.
pub fn types_source(file_name: &str) -> syn::File {
    parse(&read(
        &crate_dir("printobserver-types").join("src").join(file_name),
    ))
}

/// The names of the methods one trait declares, in declaration order.
pub fn trait_methods(file: &syn::File, trait_name: &str) -> Vec<String> {
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
                methods.push(function.sig.ident.to_string());
            }
        }
    }
    methods
}

/// The names of the variants one enum declares, in declaration order.
pub fn enum_variants(file: &syn::File, enum_name: &str) -> Vec<String> {
    for item in &file.items {
        let syn::Item::Enum(declaration) = item else {
            continue;
        };
        if declaration.ident == enum_name {
            return declaration
                .variants
                .iter()
                .map(|variant| variant.ident.to_string())
                .collect();
        }
    }
    Vec::new()
}

/// The names of the fields one struct declares, in declaration order.
pub fn struct_fields(file: &syn::File, struct_name: &str) -> Vec<String> {
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
                .filter_map(|field| field.ident.as_ref().map(ToString::to_string))
                .collect();
        }
    }
    Vec::new()
}

/// One source's tokens, as words a scanner can walk.
///
/// Rendered from the parsed tokens rather than from the text, so a name inside a
/// comment or a string literal is not a call — and with every delimiter given
/// space of its own, because a delimited group renders with its first token
/// glued to its opening bracket and every scanner below would otherwise miss
/// whatever a `vec!` or an `assert_eq!` holds.
fn words(file: &syn::File) -> Vec<String> {
    let mut rendered = file.to_token_stream().to_string();
    for delimiter in ['(', ')', '[', ']', '{', '}'] {
        rendered = rendered.replace(delimiter, &format!(" {delimiter} "));
    }
    rendered.split_whitespace().map(ToOwned::to_owned).collect()
}

/// Whether a word is an identifier.
fn is_identifier(word: &str) -> bool {
    !word.is_empty()
        && word.chars().all(|c| c.is_alphanumeric() || c == '_')
        && !word.starts_with(|c: char| c.is_numeric())
}

/// Every method name one source calls on anything.
pub fn method_calls(file: &syn::File) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for window in words(file).windows(3) {
        if window[0] == "." && window[2] == "(" && is_identifier(&window[1]) {
            names.insert(window[1].clone());
        }
    }
    names
}

/// Every field name one source reads off anything.
///
/// The same window as [`method_calls`], taking the names a call parenthesis does
/// *not* follow — which is what reading a field looks like in tokens.
pub fn field_reads(file: &syn::File) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for window in words(file).windows(3) {
        if window[0] == "." && window[2] != "(" && is_identifier(&window[1]) {
            names.insert(window[1].clone());
        }
    }
    names
}

/// Every variant of one enum that a source names by path.
pub fn variants_named(file: &syn::File, enum_name: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for window in words(file).windows(3) {
        if window[0] == enum_name && window[1] == "::" && is_identifier(&window[2]) {
            names.insert(window[2].clone());
        }
    }
    names
}
