//! The types this port declares — the three snapshots, the printer's state
//! and the adjustables — carry the stated fields, emit their schemas, and hold
//! the wire rules the contracts state.
//!
//! The `printobserver-types:schemas` graph target runs this beside the other
//! declaring crates' `schemas` tests: with `PRINTOBSERVER_SCHEMAS=write` it
//! writes every schema under `schemas/printobserver-printer-api/`, and without
//! it refuses a tree whose checked-in schema no longer matches what the types
//! generate. The port's error vocabulary emits no schema — an error reaches a
//! client as the server's own declared error shape — so a file for it under
//! this crate's directory is drift the reconciler refuses as undeclared.
//!
//! The wire rules walked here are the ones every declared type of the schema
//! set is held to: every canonical value survives a round trip, an absent
//! optional is absent rather than `null`, a reported number is its pair and
//! nothing else, and an instant is RFC 3339 at a zero offset — normalized from
//! an offset on parse and refused when it is not RFC 3339 at all.

#[path = "support/contracts.rs"]
mod contracts;
#[path = "support/schema_files.rs"]
mod schema_files;

use contracts::{contract, declared};
use printobserver_types::serde_json::{Value, json};
use printobserver_types::{WireField, wire_fields};
use schema_files::reconcile;

/// The field the contract states, as a name, what it is, and whether it is
/// required.
fn field(name: &str, descriptor: &str, required: bool) -> WireField {
    WireField {
        name: name.to_owned(),
        descriptor: descriptor.to_owned(),
        required,
    }
}

/// The schemas this crate declares, by the file name each is written under.
fn generated() -> Vec<(String, Value)> {
    declared()
        .into_iter()
        .map(|entry| (format!("{}.json", entry.name), entry.schema()))
        .collect()
}

/// The checked-in schemas of this crate are what its types generate.
#[test]
fn the_checked_in_schemas_are_what_the_types_generate() {
    let findings = reconcile("printobserver-printer-api", &generated());
    assert!(
        findings.is_empty(),
        "the checked-in schemas have drifted:\n{}",
        findings.join("\n")
    );
}

/// One field of the contract table: its name, what it is, and whether an
/// instance must carry it.
type StatedField = (&'static str, &'static str, bool);

/// The fields the declared types carry, type by type. A type whose whole wire
/// form is a scalar — an adjustable is its spelling — declares no field at
/// all, and its wire form is asserted where the accessors are.
const DECLARED_FIELDS: &[(&str, &[StatedField])] = &[
    ("Adjustable", &[]),
    (
        "HeaterSnapshot",
        &[
            ("actual_c", "Reported", false),
            ("offset_c", "Reported", false),
            ("target_c", "Reported", false),
        ],
    ),
    (
        "JobSnapshot",
        &[
            ("completion", "Reported", false),
            ("error", "string", false),
            ("estimated_print_time_s", "integer", false),
            ("file_name", "string", false),
            ("file_origin", "string", false),
            ("print_time_left_s", "integer", false),
            ("print_time_s", "integer", false),
            ("size_bytes", "integer", false),
            ("state", "PrinterState", true),
        ],
    ),
    (
        "PrinterSnapshot",
        &[
            ("bed", "HeaterSnapshot", false),
            ("chamber", "HeaterSnapshot", false),
            ("connection", "PrinterState", true),
            ("fan_percent", "Reported", false),
            ("feedrate_factor", "Reported", false),
            ("flowrate_factor", "Reported", false),
            ("observed_at", "Timestamp", true),
            ("tools", "array:HeaterSnapshot", true),
        ],
    ),
    ("PrinterState", &[("unknown", "string", true)]),
];

/// Every declared type carries exactly the fields the contract states, under
/// the name its schema carries, and the table names no other.
#[test]
fn every_declared_type_carries_exactly_the_stated_fields() {
    let entries = declared();
    assert_eq!(DECLARED_FIELDS.len(), entries.len());
    for entry in entries {
        let expected = DECLARED_FIELDS
            .iter()
            .find(|(name, _)| *name == entry.name)
            .unwrap_or_else(|| panic!("{} is declared but the contract table omits it", entry.name))
            .1;
        let expected: Vec<WireField> = expected
            .iter()
            .map(|(name, descriptor, required)| field(name, descriptor, *required))
            .collect();
        let schema = entry.schema();
        assert_eq!(
            wire_fields(&schema),
            expected,
            "{} does not carry the stated fields",
            entry.name
        );
        assert_eq!(
            schema.get("title").and_then(Value::as_str),
            Some(entry.name),
            "{} generates a schema titled otherwise",
            entry.name
        );
    }
}

/// Every declared type round-trips every one of its canonical values unchanged.
#[test]
fn every_declared_type_round_trips_its_canonical_values() {
    for entry in declared() {
        for value in entry.samples() {
            let round = entry
                .round_trip(value.clone())
                .unwrap_or_else(|error| panic!("{}: {error}", entry.name));
            assert_eq!(round, value, "{} does not survive a round trip", entry.name);
        }
    }
}

/// An optional field that is absent is absent from the serialized object.
///
/// Never `null`, never a zero, never an empty string: an optional field absent
/// means the source did not report it.
#[test]
fn an_absent_optional_is_absent_from_the_serialized_object() {
    let mut checked = 0_usize;
    for entry in declared() {
        let optional: Vec<String> = wire_fields(&entry.schema())
            .into_iter()
            .filter(|field| !field.required)
            .map(|field| field.name)
            .collect();
        let minimal = entry.minimal();
        let Some(object) = minimal.as_object() else {
            continue;
        };
        for name in optional {
            assert!(
                !object.contains_key(&name),
                "{}'s minimal value carries {name} as {:?}",
                entry.name,
                object.get(&name)
            );
            checked += 1;
        }
    }
    assert_eq!(
        checked, 16,
        "the sixteen optional fields were not all checked"
    );
}

/// Every reported field of every full value serializes as its pair, and
/// nothing else.
#[test]
fn a_reported_value_serializes_as_its_pair() {
    let mut checked = 0_usize;
    for entry in declared() {
        let full = entry.full();
        for field in wire_fields(&entry.schema()) {
            if field.descriptor != "Reported" {
                continue;
            }
            let reported = full[&field.name]
                .as_object()
                .expect("a reported value is an object");
            assert!(reported.get("value").is_some_and(Value::is_number));
            assert!(reported.get("out_of_range").is_some_and(Value::is_boolean));
            assert_eq!(
                reported.len(),
                2,
                "{}.{} carries {reported:?}",
                entry.name,
                field.name
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 7, "the seven reported fields were not all checked");
}

/// The one instant a snapshot carries serializes at a zero offset, normalizes
/// an offset on parse, and refuses what is not RFC 3339.
#[test]
fn the_observed_instant_is_rfc_3339_at_a_zero_offset() {
    let entry = contract("PrinterSnapshot");
    let full = entry.full();
    let text = full["observed_at"]
        .as_str()
        .expect("an instant is a string");
    assert!(text.ends_with('Z'), "observed_at is {text}");

    let mut offset = full.clone();
    offset["observed_at"] = json!("2026-03-01T13:00:00+01:00");
    let round = entry
        .round_trip(offset)
        .unwrap_or_else(|error| panic!("an offset was refused: {error}"));
    assert_eq!(
        round["observed_at"].as_str(),
        Some("2026-03-01T12:00:00Z"),
        "observed_at was not re-emitted at a zero offset"
    );

    for refused in ["2026-03-01T12:00:00", "the first of March"] {
        let mut value = full.clone();
        value["observed_at"] = json!(refused);
        assert!(
            entry.round_trip(value).is_err(),
            "observed_at accepted {refused}"
        );
    }
}

/// A value of no declared type is refused by every declared type.
#[test]
fn nothing_parses_a_value_of_no_declared_type() {
    let nonsense = json!({ "printobserver": "no type declares this field" });
    for entry in declared() {
        assert!(
            entry.round_trip(nonsense.clone()).is_err(),
            "{} accepted a value of no declared type",
            entry.name
        );
    }
}
