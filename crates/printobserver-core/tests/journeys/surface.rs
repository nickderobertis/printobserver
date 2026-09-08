//! This crate's own surface: what it depends on, and what can reach a printer.
//!
//! The manifest declares the type crate and the four port crates and nothing
//! else, and its tests reach no implementation either. And **no printer-port
//! method admits a byte sequence or an unvalidated string** — neither as a
//! parameter's own type nor, following each declared field and each variant
//! payload in turn, anywhere in the closure reachable from it. The sole
//! exception is the `FileName` newtype the contracts declare, which exactly one
//! method takes and which refuses anything but a name.
//!
//! So this crate has no surface at all through which a command string could
//! reach a printer, which is what makes that safety property structural rather
//! than a habit; and because the exemption is a **type** rather than a
//! parameter, it carries its own validation and a rename cannot widen it.
//!
//! Enumerating only the immediate parameter types is what the closure walk
//! exists to improve on: a method taking a struct that carries a `String` lets
//! command data through while a manifest read and a direct-parameter check both
//! pass.

use crate::source::{
    Universe, crate_dir, crate_modules, manifest_dependencies, parse, read, trait_method_params,
};

/// The crate whose surface these checks read.
const CRATE: &str = "printobserver-core";

/// The one newtype a printer-port method may take a string inside.
const EXEMPT: &str = "FileName";

/// The one method that newtype may be taken by.
const EXEMPT_METHOD: &str = "start";

/// The printer port's own source.
fn port_source() -> String {
    read(
        &crate_dir("printobserver-printer-api")
            .join("src")
            .join("lib.rs"),
    )
}

/// A universe of the declarations a printer-port parameter can reach.
fn universe(extra: &str) -> Universe {
    let mut sources: Vec<syn::File> = crate_modules("printobserver-types")
        .iter()
        .map(|(_, source)| parse(source))
        .collect();
    sources.push(parse(extra));
    Universe::new(sources)
}

/// Every way one port declaration lets caller-supplied text or bytes through.
///
/// Empty means the declaration holds the property.
fn findings_for(source: &str) -> Vec<String> {
    let file = parse(source);
    let params = trait_method_params(&file, "PrinterPort");
    let mut findings = Vec::new();
    let roots: Vec<(String, String)> = params
        .iter()
        .map(|(method, name, ty)| (format!("{method}({name})"), ty.clone()))
        .collect();
    let walked = universe(source).closure(&roots);
    for path in &walked.bytes {
        findings.push(format!("{path} reaches a byte sequence"));
    }
    for path in &walked.strings {
        if !path.contains(EXEMPT) {
            findings.push(format!("{path} reaches a string outside {EXEMPT}"));
        }
    }
    for (method, name, ty) in &params {
        if ty.contains(EXEMPT) && method != EXEMPT_METHOD {
            findings.push(format!(
                "{method}({name}) takes {EXEMPT}, and only {EXEMPT_METHOD} may"
            ));
        }
    }
    if !params
        .iter()
        .any(|(method, _, ty)| method == EXEMPT_METHOD && ty.contains(EXEMPT))
    {
        findings.push(format!("{EXEMPT_METHOD} does not take {EXEMPT}"));
    }
    findings
}

/// Assert a fixture is refused, and refused for the reason it was built for.
#[track_caller]
fn refused(findings: &[String], expected: &str) {
    assert!(
        findings.iter().any(|finding| finding.contains(expected)),
        "expected a finding naming {expected:?}, found {findings:?}"
    );
}

/// The manifest declares the type crate and the four ports and nothing else.
#[test]
fn the_manifest_declares_the_type_crate_and_the_four_ports_and_nothing_else() {
    assert_eq!(
        manifest_dependencies(CRATE, "dependencies"),
        vec![
            "printobserver-printer-api".to_owned(),
            "printobserver-store-api".to_owned(),
            "printobserver-supervisor-api".to_owned(),
            "printobserver-types".to_owned(),
            "printobserver-vision-api".to_owned(),
        ]
    );
}

/// This crate's tests reach no crate of this workspace either.
///
/// The test-only readers of this crate's own sources are not crates of this
/// workspace, so no implementation crate can reach core through its tests: the
/// fakes here are this crate's own and nothing else implements a port.
#[test]
fn this_crates_tests_reach_no_crate_of_this_workspace() {
    let declared = manifest_dependencies(CRATE, "dev-dependencies");
    let workspace: Vec<&String> = declared
        .iter()
        .filter(|name| name.starts_with("printobserver"))
        .collect();
    assert_eq!(
        workspace,
        Vec::<&String>::new(),
        "this crate's tests declare a crate of this workspace"
    );
}

/// No printer-port method admits a byte sequence or an unvalidated string.
#[test]
fn no_printer_port_method_admits_a_byte_sequence_or_an_unvalidated_string() {
    assert_eq!(findings_for(&port_source()), Vec::<String>::new());
}

/// The closure reaches exactly one string, and it is the one the newtype wraps.
#[test]
fn the_only_string_in_the_closure_is_the_one_the_newtype_wraps() {
    let source = port_source();
    let file = parse(&source);
    let roots: Vec<(String, String)> = trait_method_params(&file, "PrinterPort")
        .iter()
        .map(|(method, name, ty)| (format!("{method}({name})"), ty.clone()))
        .collect();
    let walked = universe(&source).closure(&roots);

    assert_eq!(walked.bytes, Vec::<String>::new());
    assert_eq!(walked.strings.len(), 1, "{:?}", walked.strings);
    assert!(walked.strings[0].contains(EXEMPT));
    assert!(walked.reached.contains(EXEMPT));
}

/// A method taking a bare string directly is refused.
#[test]
fn a_method_taking_a_bare_string_is_refused() {
    let fixture = "pub trait PrinterPort { \
        fn start(&self, file_name: FileName); \
        fn run(&self, command: String); }";
    refused(&findings_for(fixture), "reaches a string outside FileName");
}

/// A method taking a struct that carries a string field is refused.
#[test]
fn a_method_taking_a_struct_carrying_a_string_is_refused() {
    let fixture = "pub struct Job { pub gcode: String } \
        pub trait PrinterPort { \
        fn start(&self, file_name: FileName); \
        fn run(&self, job: Job); }";
    refused(&findings_for(fixture), "reaches a string outside FileName");
}

/// A method taking an enum whose variant carries a byte sequence is refused.
#[test]
fn a_method_taking_an_enum_carrying_bytes_is_refused() {
    let fixture = "pub enum Payload { Raw { bytes: Vec<u8> } } \
        pub trait PrinterPort { \
        fn start(&self, file_name: FileName); \
        fn send(&self, payload: Payload); }";
    refused(&findings_for(fixture), "reaches a byte sequence");
}

/// A method other than `start` taking the newtype is refused.
#[test]
fn a_second_method_taking_the_newtype_is_refused() {
    let fixture = "pub trait PrinterPort { \
        fn start(&self, file_name: FileName); \
        fn preheat_for(&self, file_name: FileName); }";
    refused(
        &findings_for(fixture),
        "preheat_for(file_name) takes FileName",
    );
}

/// `start` taking a bare string in place of the newtype is refused.
#[test]
fn start_taking_a_bare_string_in_place_of_the_newtype_is_refused() {
    let fixture = "pub trait PrinterPort { fn start(&self, file_name: String); }";
    let findings = findings_for(fixture);
    refused(&findings, "reaches a string outside FileName");
    refused(&findings, "start does not take FileName");
}
