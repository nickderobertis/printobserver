//! The journeys do not fall behind the contracts they are about.
//!
//! Three of this crate's claims are claims about coverage of a contract this
//! crate does not own — every port method driven, every error variant produced,
//! every snapshot field walked — and a suite that covers most of a contract is
//! one whose gaps nobody can name. So each check below reads the declarations
//! themselves and the journey's own calls, rather than a list maintained beside
//! either, and each is driven against a fixture carrying exactly the gap it
//! exists to find, so what refuses the fixture is what reads the committed pair.

use std::collections::BTreeSet;

use crate::surface::{
    enum_variants, field_reads, method_calls, parse, port, struct_fields, test_source,
    trait_methods, types_source, variants_named,
};

/// The two methods of the printer port that read rather than act.
const READING_METHODS: [&str; 2] = ["snapshot", "job"];

/// Everything declared that the journey does not cover.
fn uncovered(declared: &[String], covered: &BTreeSet<String>) -> Vec<String> {
    declared
        .iter()
        .filter(|name| !covered.contains(*name))
        .cloned()
        .collect()
}

/// The redaction journey drives every method the printer port declares.
#[test]
fn the_redaction_universe_drives_every_method_the_port_declares() {
    let declared = trait_methods(&port(), "PrinterPort");
    assert!(
        declared.len() >= 11,
        "the reader found only {} port methods, which is not the port",
        declared.len()
    );

    let driven = method_calls(&test_source("journeys/redaction.rs"));

    assert_eq!(
        uncovered(&declared, &driven),
        Vec::<String>::new(),
        "the redaction universe does not drive every method the port declares"
    );
}

/// A port carrying a method the redaction universe does not drive.
const FIXTURE_PORT_WITH_AN_UNDRIVEN_METHOD: &str = r"
pub trait PrinterPort: Send + Sync {
    fn snapshot(&self) -> BoxFuture<'_, Result<PrinterSnapshot, PrinterError>>;
    fn set_chamber_target_c(&self, target_c: f64) -> BoxFuture<'_, Result<(), PrinterError>>;
}
";

/// The check names a port method the redaction universe never drives.
#[test]
fn the_check_refuses_a_port_method_the_redaction_universe_does_not_drive() {
    let declared = trait_methods(&parse(FIXTURE_PORT_WITH_AN_UNDRIVEN_METHOD), "PrinterPort");

    assert_eq!(
        uncovered(
            &declared,
            &method_calls(&test_source("journeys/redaction.rs"))
        ),
        vec!["set_chamber_target_c".to_owned()]
    );
}

/// The redaction journey renders a value of every variant the error declares.
#[test]
fn the_redaction_universe_renders_every_error_variant_declared() {
    let declared = enum_variants(&port(), "PrinterError");
    assert!(
        declared.len() >= 6,
        "the reader found only {} error variants, which is not the vocabulary",
        declared.len()
    );

    let rendered = variants_named(&test_source("journeys/redaction.rs"), "PrinterError");

    assert_eq!(
        uncovered(&declared, &rendered),
        Vec::<String>::new(),
        "the redaction universe does not render every variant the error declares"
    );
}

/// An error vocabulary carrying a variant nothing renders.
const FIXTURE_ERROR_WITH_AN_UNRENDERED_VARIANT: &str = r"
pub enum PrinterError {
    Unreachable { detail: String },
    Overheating { detail: String },
}
";

/// The check names an error variant the redaction universe never renders.
#[test]
fn the_check_refuses_an_error_variant_the_redaction_universe_does_not_render() {
    let declared = enum_variants(
        &parse(FIXTURE_ERROR_WITH_AN_UNRENDERED_VARIANT),
        "PrinterError",
    );

    assert_eq!(
        uncovered(
            &declared,
            &variants_named(&test_source("journeys/redaction.rs"), "PrinterError")
        ),
        vec!["Overheating".to_owned()]
    );
}

/// Every field the two snapshot contracts declare.
fn snapshot_fields() -> Vec<String> {
    let printer = types_source("printer.rs");
    let mut fields = struct_fields(&printer, "PrinterSnapshot");
    fields.extend(struct_fields(&printer, "JobSnapshot"));
    fields.extend(struct_fields(&printer, "HeaterSnapshot"));
    fields
}

/// The reading walk covers every field the two snapshot contracts declare.
#[test]
fn the_reading_walk_covers_every_field_the_snapshot_contracts_declare() {
    let declared = snapshot_fields();
    assert!(
        declared.len() >= 20,
        "the reader found only {} snapshot fields, which is not the contracts",
        declared.len()
    );

    let walked = field_reads(&test_source("live/reading.rs"));

    assert_eq!(
        uncovered(&declared, &walked),
        Vec::<String>::new(),
        "the reading walk does not cover every field the snapshot contracts declare"
    );
}

/// A snapshot contract carrying a field nothing walks.
const FIXTURE_SNAPSHOT_WITH_AN_UNWALKED_FIELD: &str = r"
pub struct PrinterSnapshot {
    pub connection: PrinterState,
    pub chamber_fan_percent: Option<Reported<f64>>,
}
";

/// The check names a snapshot field the reading walk never covers.
#[test]
fn the_check_refuses_a_snapshot_field_the_reading_walk_does_not_cover() {
    let declared = struct_fields(
        &parse(FIXTURE_SNAPSHOT_WITH_AN_UNWALKED_FIELD),
        "PrinterSnapshot",
    );

    assert_eq!(
        uncovered(&declared, &field_reads(&test_source("live/reading.rs"))),
        vec!["chamber_fan_percent".to_owned()]
    );
}

/// The acting walk drives every action method the printer port declares.
#[test]
fn the_acting_walk_drives_every_action_method_the_port_declares() {
    let declared: Vec<String> = trait_methods(&port(), "PrinterPort")
        .into_iter()
        .filter(|name| !READING_METHODS.contains(&name.as_str()))
        .collect();
    assert!(
        declared.len() >= 9,
        "the reader found only {} action methods, which is not the port",
        declared.len()
    );

    let driven = method_calls(&test_source("live/acting.rs"));

    assert_eq!(
        uncovered(&declared, &driven),
        Vec::<String>::new(),
        "the acting walk does not drive every action method the port declares"
    );
}

/// The check names an action method the acting walk never drives.
#[test]
fn the_check_refuses_an_action_method_the_acting_walk_does_not_drive() {
    let declared: Vec<String> =
        trait_methods(&parse(FIXTURE_PORT_WITH_AN_UNDRIVEN_METHOD), "PrinterPort")
            .into_iter()
            .filter(|name| !READING_METHODS.contains(&name.as_str()))
            .collect();

    assert_eq!(
        uncovered(&declared, &method_calls(&test_source("live/acting.rs"))),
        vec!["set_chamber_target_c".to_owned()]
    );
}

/// The failing walk names every variant the error vocabulary declares, either
/// by producing it against this target or by recording it as one this target
/// cannot produce, with the reason.
#[test]
fn the_failing_walk_names_every_error_variant_declared() {
    let declared = enum_variants(&port(), "PrinterError");

    let named = variants_named(&test_source("live/failing.rs"), "PrinterError");

    assert_eq!(
        uncovered(&declared, &named),
        Vec::<String>::new(),
        "the failing walk neither produces nor records a variant the error declares"
    );
}

/// The check names an error variant the failing walk neither produces nor
/// records.
#[test]
fn the_check_refuses_an_error_variant_the_failing_walk_does_not_name() {
    let declared = enum_variants(
        &parse(FIXTURE_ERROR_WITH_AN_UNRENDERED_VARIANT),
        "PrinterError",
    );

    assert_eq!(
        uncovered(
            &declared,
            &variants_named(&test_source("live/failing.rs"), "PrinterError")
        ),
        vec!["Overheating".to_owned()]
    );
}
