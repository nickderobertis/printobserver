//! The printer port declares exactly the surface the contract states.
//!
//! These tests read this crate's own declarations rather than a table
//! maintained beside them, and every fixture they are driven against is a
//! source snippet read by the same reader. The reader is the type crate's
//! `tests/support/surface.rs`, included here by `#[path]`: a port crate
//! declares the type crate as its only workspace dependency, and a reader of
//! Rust sources is a claim about no crate in particular.

#[path = "support/closure.rs"]
mod closure;
#[path = "../../printobserver-types/tests/support/surface.rs"]
pub mod surface;

use closure::Universe;
use surface::{
    Field, Method, Param, Variant, crate_dir, crate_source, crate_sources, enum_variants, parse,
    trait_methods, workspace_dependency_names,
};

/// This crate's own name.
const CRATE: &str = "printobserver-printer-api";

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

/// Every method the printer port is stated to declare, and no other.
#[test]
fn printer_port_declares_exactly_the_stated_methods() {
    let source = parse(&crate_source(CRATE));
    let expected = vec![
        method(
            "snapshot",
            &[],
            "BoxFuture<'_,Result<PrinterSnapshot,PrinterError>>",
        ),
        method("job", &[], "BoxFuture<'_,Result<JobSnapshot,PrinterError>>"),
        method(
            "start",
            &[("file_name", "FileName")],
            "BoxFuture<'_,Result<(),PrinterError>>",
        ),
        method("pause", &[], "BoxFuture<'_,Result<(),PrinterError>>"),
        method("resume", &[], "BoxFuture<'_,Result<(),PrinterError>>"),
        method("cancel", &[], "BoxFuture<'_,Result<(),PrinterError>>"),
        method(
            "set_feedrate_factor",
            &[("factor", "f64")],
            "BoxFuture<'_,Result<(),PrinterError>>",
        ),
        method(
            "set_flowrate_factor",
            &[("factor", "f64")],
            "BoxFuture<'_,Result<(),PrinterError>>",
        ),
        method(
            "set_tool_target_c",
            &[("tool", "i64"), ("target_c", "f64")],
            "BoxFuture<'_,Result<(),PrinterError>>",
        ),
        method(
            "set_bed_target_c",
            &[("target_c", "f64")],
            "BoxFuture<'_,Result<(),PrinterError>>",
        ),
        method(
            "set_fan_percent",
            &[("percent", "f64")],
            "BoxFuture<'_,Result<(),PrinterError>>",
        ),
    ];
    assert_eq!(trait_methods(&source, "PrinterPort"), expected);
}

/// Every variant the printer port's error vocabulary is stated to carry.
#[test]
fn printer_error_carries_exactly_the_stated_variants() {
    let source = parse(&crate_source(CRATE));
    let expected = vec![
        variant("Unreachable", &[("detail", "String")]),
        variant("Unauthorized", &[("detail", "String")]),
        variant("Refused", &[("status", "u16"), ("detail", "String")]),
        variant("StateConflict", &[("detail", "String")]),
        variant("Unsupported", &[("adjustable", "Adjustable")]),
        variant("Malformed", &[("detail", "String")]),
    ];
    assert_eq!(enum_variants(&source, "PrinterError"), expected);
}

/// Every port method is asynchronous, in the one shape a trait object carries.
#[test]
fn every_port_method_answers_a_boxed_future() {
    let source = parse(&crate_source(CRATE));
    let methods = trait_methods(&source, "PrinterPort");
    assert!(!methods.is_empty(), "PrinterPort declares no method");
    for declared in methods {
        assert!(
            declared.returns.starts_with("BoxFuture<'_,Result<"),
            "PrinterPort::{} answers {}, which is neither asynchronous nor a Result",
            declared.name,
            declared.returns
        );
    }
}

/// The port's error vocabulary carries no not-yet-implemented variant.
#[test]
fn the_port_error_carries_no_not_yet_implemented_variant() {
    let source = parse(&crate_source(CRATE));
    for declared in enum_variants(&source, "PrinterError") {
        let name = declared.name.to_lowercase();
        assert!(
            !(name.contains("notimplemented")
                || name.contains("unimplemented")
                || name.contains("todo")),
            "PrinterError carries {}, a variant no real implementation can produce",
            declared.name
        );
    }
}

/// The closure a trait's parameters reach, resolved against the type crate
/// and this crate's own declarations.
fn parameter_closure(fixture: Option<&str>, trait_name: &str) -> closure::Findings {
    let mut sources = crate_sources("printobserver-types");
    sources.extend(crate_sources(CRATE));
    let declaration = match fixture {
        Some(text) => parse(text),
        None => parse(&crate_source(CRATE)),
    };
    sources.push(declaration.clone());
    let roots: Vec<(String, String)> = trait_methods(&declaration, trait_name)
        .into_iter()
        .flat_map(|declared| {
            declared.params.into_iter().map(move |param| {
                (
                    format!("{}::{}({})", trait_name, declared.name, param.name),
                    param.ty,
                )
            })
        })
        .collect();
    Universe::new(sources).closure(&roots)
}

/// The one fixture shape a `FileName` exemption keyed on a parameter would let
/// through, and the four a no-string rule read only at the signature would.
const FIXTURE_BARE_STRING: &str = r"
pub trait Fixture {
    fn start(&self, file_name: String) -> BoxFuture<'_, Result<(), PrinterError>>;
}
";

/// A fixture whose parameter is a struct carrying a string.
const FIXTURE_STRUCT_WITH_STRING: &str = r"
pub struct Instruction {
    pub gcode: String,
}
pub trait Fixture {
    fn start(&self, instruction: Instruction) -> BoxFuture<'_, Result<(), PrinterError>>;
}
";

/// A fixture whose parameter is an enum carrying a byte sequence.
const FIXTURE_ENUM_WITH_BYTES: &str = r"
pub enum Blob {
    Raw(Vec<u8>),
}
pub trait Fixture {
    fn start(&self, blob: Blob) -> BoxFuture<'_, Result<(), PrinterError>>;
}
";

/// A fixture in which a method other than `start` takes a `FileName`.
const FIXTURE_SECOND_FILE_NAME: &str = r"
pub trait Fixture {
    fn start(&self, file_name: FileName) -> BoxFuture<'_, Result<(), PrinterError>>;
    fn cancel(&self, file_name: FileName) -> BoxFuture<'_, Result<(), PrinterError>>;
}
";

/// The printer port admits no byte sequence, and one string in one place.
#[test]
fn printer_port_closure_carries_no_bytes_and_one_string() {
    let findings = parameter_closure(None, "PrinterPort");
    assert!(
        findings.bytes.is_empty(),
        "the closure reaches bytes at {:?}",
        findings.bytes
    );
    assert_eq!(
        findings.strings.len(),
        1,
        "the closure reaches {:?}",
        findings.strings
    );
    assert!(
        findings.strings[0].ends_with("/FileName.0"),
        "the one string is {}, not the one FileName wraps",
        findings.strings[0]
    );
    assert!(
        findings.reached.contains("FileName"),
        "the closure does not reach FileName at all"
    );
}

/// Exactly one printer-port method takes a `FileName`, and it is `start`.
#[test]
fn only_start_takes_a_file_name() {
    let source = parse(&crate_source(CRATE));
    let taking: Vec<(String, usize)> = trait_methods(&source, "PrinterPort")
        .into_iter()
        .map(|declared| {
            let count = declared
                .params
                .iter()
                .filter(|param| param.ty.contains("FileName"))
                .count();
            (declared.name, count)
        })
        .filter(|(_, count)| *count > 0)
        .collect();
    assert_eq!(taking, vec![("start".to_owned(), 1)]);
}

/// A fixture carrying a string or a byte sequence anywhere in the closure is
/// refused, which is what makes the committed trait's acceptance mean something.
#[test]
fn the_closure_walk_refuses_every_fixture_it_must() {
    for (label, fixture) in [
        ("a bare string parameter", FIXTURE_BARE_STRING),
        ("a struct carrying a string", FIXTURE_STRUCT_WITH_STRING),
    ] {
        let findings = parameter_closure(Some(fixture), "Fixture");
        assert!(
            findings
                .strings
                .iter()
                .any(|path| !path.ends_with("/FileName.0")),
            "{label} was not refused"
        );
    }
    let findings = parameter_closure(Some(FIXTURE_ENUM_WITH_BYTES), "Fixture");
    assert!(
        !findings.bytes.is_empty(),
        "an enum carrying a byte sequence was not refused"
    );

    let fixture = parse(FIXTURE_SECOND_FILE_NAME);
    let taking: Vec<String> = trait_methods(&fixture, "Fixture")
        .into_iter()
        .filter(|declared| {
            declared
                .params
                .iter()
                .any(|param| param.ty.contains("FileName"))
        })
        .map(|declared| declared.name)
        .collect();
    assert_eq!(
        taking,
        vec!["start".to_owned(), "cancel".to_owned()],
        "a second method taking a FileName was not seen"
    );

    let fixture = parse(FIXTURE_BARE_STRING);
    let start = trait_methods(&fixture, "Fixture")
        .into_iter()
        .find(|declared| declared.name == "start")
        .expect("the fixture declares start");
    assert!(
        !start
            .params
            .iter()
            .any(|param| param.ty.contains("FileName")),
        "a start taking a bare string was read as taking the newtype"
    );
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

/// A fixture manifest naming an implementation crate beside the type crate.
const FIXTURE_MANIFEST_WITH_AN_ADAPTER: &str = r#"
[dependencies]
printobserver-types = { path = "../printobserver-types", version = "0.2.0" }

[dev-dependencies]
printobserver-octoprint = { path = "../printobserver-octoprint", version = "0.2.0" }
syn = "3"
"#;

/// The manifest reading sees a workspace edge in any table, and only those.
#[test]
fn the_manifest_reading_refuses_a_second_workspace_crate_in_any_table() {
    assert_eq!(
        workspace_dependency_names(FIXTURE_MANIFEST_WITH_AN_ADAPTER),
        vec![
            "printobserver-octoprint".to_owned(),
            "printobserver-types".to_owned()
        ]
    );
}
