//! The vision port declares exactly the surface the contract states.
//!
//! These tests read this crate's own declarations rather than a table
//! maintained beside them, and every fixture they are driven against is a
//! source snippet read by the same reader. The reader is the type crate's
//! `tests/support/surface.rs`, included here by `#[path]`: a port crate
//! declares the type crate as its only workspace dependency, and a reader of
//! Rust sources is a claim about no crate in particular.

#[path = "../../printobserver-types/tests/support/surface.rs"]
pub mod surface;

use surface::{
    Field, Method, Param, Variant, crate_dir, crate_source, enum_variants, parse, trait_methods,
    workspace_dependency_names,
};

/// This crate's own name.
const CRATE: &str = "printobserver-vision-api";

/// The method the contract states, as a name, its parameters and its answer.
fn method(name: &str, params: &[(&str, &str)], returns: &str) -> Method {
    Method {
        name: name.to_owned(),
        params: params
            .iter()
            .map(|(name, ty)| Param {
                name: (*name).to_owned(),
                ty: (*ty).to_owned(),
            })
            .collect(),
        returns: returns.to_owned(),
    }
}

/// The variant the contract states, as a name and its payload's fields.
fn variant(name: &str, fields: &[(&str, &str)]) -> Variant {
    Variant {
        name: name.to_owned(),
        fields: fields
            .iter()
            .map(|(name, ty)| Field {
                name: (*name).to_owned(),
                ty: (*ty).to_owned(),
            })
            .collect(),
    }
}

/// Every method the vision port is stated to declare, and no other.
#[test]
fn vision_port_declares_exactly_the_stated_methods() {
    let source = parse(&crate_source(CRATE));
    let expected = vec![
        method(
            "normalize",
            &[("body", "RawBytes"), ("content_type", "Option<String>")],
            "BoxFuture<'_,Result<NormalizedAlert,VisionError>>",
        ),
        method(
            "fetch_image",
            &[("source_url", "String")],
            "BoxFuture<'_,Result<FetchedImage,VisionError>>",
        ),
    ];
    assert_eq!(trait_methods(&source, "VisionPort"), expected);
}

/// Every variant the vision port's error vocabulary is stated to carry.
#[test]
fn vision_error_carries_exactly_the_stated_variants() {
    let source = parse(&crate_source(CRATE));
    let expected = vec![
        variant("Malformed", &[("detail", "String"), ("raw", "RawBytes")]),
        variant("TimedOut", &[]),
        variant("TooLarge", &[("limit", "i64")]),
        variant("UnacceptableContentType", &[("content_type", "String")]),
        variant("Unreachable", &[("detail", "String")]),
    ];
    assert_eq!(enum_variants(&source, "VisionError"), expected);
}

/// Every port method is asynchronous, in the one shape a trait object carries.
#[test]
fn every_port_method_answers_a_boxed_future() {
    let source = parse(&crate_source(CRATE));
    let methods = trait_methods(&source, "VisionPort");
    assert!(!methods.is_empty(), "VisionPort declares no method");
    for declared in methods {
        assert!(
            declared.returns.starts_with("BoxFuture<'_,Result<"),
            "VisionPort::{} answers {}, which is neither asynchronous nor a Result",
            declared.name,
            declared.returns
        );
    }
}

/// The port's error vocabulary carries no not-yet-implemented variant.
#[test]
fn the_port_error_carries_no_not_yet_implemented_variant() {
    let source = parse(&crate_source(CRATE));
    for declared in enum_variants(&source, "VisionError") {
        let name = declared.name.to_lowercase();
        assert!(
            !(name.contains("notimplemented")
                || name.contains("unimplemented")
                || name.contains("todo")),
            "VisionError carries {}, a variant no real implementation can produce",
            declared.name
        );
    }
}

/// This crate names the type crate as its only workspace dependency, across
/// every dependency table: a port that named an implementation would stop
/// being a port.
#[test]
fn the_manifest_names_the_type_crate_and_no_other_workspace_crate() {
    let manifest = std::fs::read_to_string(crate_dir(CRATE).join("Cargo.toml"))
        .expect("the manifest is readable");
    assert_eq!(
        workspace_dependency_names(&manifest),
        vec!["printobserver-types".to_owned()]
    );
}
