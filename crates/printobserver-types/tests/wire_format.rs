//! The wire representation of every type is the one the contract states.
//!
//! These assertions are on the *serialized form* rather than on the value that
//! survives a round trip: a representation that survives a round trip while
//! serializing in another form would satisfy the first and fail the second.
//!
//! Beside the corpus of well-formed values stands an adversarial walk, because
//! a corpus establishes nothing about values it does not contain. The walk is
//! anchored to the declared types rather than to a sample: it reads which
//! fields are identifiers and which are instants off the schema each type
//! generates, so a declared type that gains a field of either kind is driven
//! without anyone remembering to add it.

use printobserver_types::contract::{TypeContract, declared};
use printobserver_types::{WireField, wire_fields};
use serde_json::Value;

/// The definitions one generated schema carries.
fn definitions(schema: &Value) -> &serde_json::Map<String, Value> {
    static EMPTY: std::sync::LazyLock<serde_json::Map<String, Value>> =
        std::sync::LazyLock::new(serde_json::Map::new);
    schema
        .get("$defs")
        .and_then(Value::as_object)
        .unwrap_or(&EMPTY)
}

/// The `format` a field's referenced definition declares, if it declares one.
fn format_of(schema: &Value, field: &WireField) -> Option<String> {
    if field.descriptor == "Reported" {
        return Some("reported".to_owned());
    }
    let definition = definitions(schema).get(&field.descriptor)?;
    definition
        .get("format")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Every field of one type whose declared type carries this format.
fn fields_with_format(entry: &TypeContract, wanted: &str) -> Vec<String> {
    let schema = entry.schema();
    wire_fields(&schema)
        .into_iter()
        .filter(|field| format_of(&schema, field).is_some_and(|format| format == wanted))
        .map(|field| field.name)
        .collect()
}

/// Whether a string is the one identifier spelling this system mints.
fn is_lowercase_hyphenated_version_seven(text: &str) -> bool {
    text.len() == 36
        && text
            .chars()
            .all(|character| !character.is_ascii_uppercase())
        && text.as_bytes()[14] == b'7'
        && [8, 13, 18, 23]
            .iter()
            .all(|index| text.as_bytes()[*index] == b'-')
        && text
            .chars()
            .filter(|character| *character != '-')
            .all(|c| c.is_ascii_hexdigit())
}

/// The samples of one type that carry one field.
fn samples_carrying(entry: &TypeContract, field: &str) -> Vec<Value> {
    entry
        .samples()
        .into_iter()
        .filter(|value| value.get(field).is_some_and(|held| !held.is_null()))
        .collect()
}

/// Every sample of one type that must carry the named field carries it.
///
/// A field the schema declares and no sample carries is a field this walk would
/// silently never drive, so it is refused here.
fn driven_samples(entry: &TypeContract, field: &str) -> Vec<Value> {
    let carrying = samples_carrying(entry, field);
    assert!(
        !carrying.is_empty(),
        "{}.{field} is declared and no canonical value carries it, so nothing drives it",
        entry.name
    );
    carrying
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
        let schema = entry.schema();
        let optional: Vec<String> = wire_fields(&schema)
            .into_iter()
            .filter(|field| !field.required)
            .map(|field| field.name)
            .collect();
        if optional.is_empty() {
            continue;
        }
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
    assert!(checked > 0, "no optional field was checked at all");
}

/// Every identifier serializes as a lowercase hyphenated version 7 UUID.
#[test]
fn every_identifier_serializes_in_the_one_spelling() {
    let mut checked = 0_usize;
    for entry in declared() {
        let full = entry.full();
        if let Some(text) = full.as_str()
            && entry.name.ends_with("Id")
        {
            assert!(
                is_lowercase_hyphenated_version_seven(text),
                "{} is {text}",
                entry.name
            );
            checked += 1;
        }
        for name in fields_with_format(&entry, "uuid") {
            for sample in driven_samples(&entry, &name) {
                let text = sample[&name].as_str().expect("an identifier is a string");
                assert!(
                    is_lowercase_hyphenated_version_seven(text),
                    "{}.{name} is {text}",
                    entry.name
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 0, "no identifier was checked at all");
}

/// Every timestamp serializes as RFC 3339 at a zero offset.
#[test]
fn every_timestamp_serializes_at_a_zero_offset() {
    let mut checked = 0_usize;
    for entry in declared() {
        for name in fields_with_format(&entry, "date-time") {
            for sample in driven_samples(&entry, &name) {
                let text = sample[&name].as_str().expect("an instant is a string");
                assert!(
                    text.ends_with('Z'),
                    "{}.{name} is {text}, which is not a zero offset",
                    entry.name
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 0, "no timestamp was checked at all");
}

/// A reported value serializes as its pair, and nothing else.
#[test]
fn a_reported_value_serializes_as_its_pair() {
    let mut checked = 0_usize;
    for entry in declared() {
        for name in fields_with_format(&entry, "reported") {
            for sample in driven_samples(&entry, &name) {
                let reported = sample[&name]
                    .as_object()
                    .expect("a reported value is an object");
                assert!(reported.get("value").is_some_and(Value::is_number));
                assert!(reported.get("out_of_range").is_some_and(Value::is_boolean));
                assert_eq!(
                    reported.len(),
                    2,
                    "{}.{name} carries {reported:?}",
                    entry.name
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 0, "no reported value was checked at all");
}

/// The identifier spellings this system refuses on parse.
const REFUSED_IDENTIFIERS: [&str; 4] = [
    "0191f0a0-0000-4000-8000-000000000001",
    "0191F0A0-0000-7000-8000-000000000001",
    "{0191f0a0-0000-7000-8000-000000000001}",
    "not-a-uuid",
];

/// Every identifier field refuses a spelling this system does not mint.
#[test]
fn every_identifier_field_refuses_a_spelling_this_system_does_not_mint() {
    let mut driven = 0_usize;
    for entry in declared() {
        let full = entry.full();
        for name in fields_with_format(&entry, "uuid") {
            for sample in driven_samples(&entry, &name) {
                for refused in REFUSED_IDENTIFIERS {
                    let mut value = sample.clone();
                    value[&name] = Value::String(refused.to_owned());
                    let answer = entry.round_trip(value);
                    assert!(
                        answer.is_err(),
                        "{}.{name} accepted {refused} and answered {answer:?}",
                        entry.name
                    );
                    driven += 1;
                }
            }
        }
        if full.is_string() && entry.name.ends_with("Id") {
            for refused in REFUSED_IDENTIFIERS {
                let answer = entry.round_trip(Value::String(refused.to_owned()));
                assert!(answer.is_err(), "{} accepted {refused}", entry.name);
                driven += 1;
            }
        }
    }
    assert!(driven > 0, "no identifier field was driven at all");
}

/// Every instant field normalizes an offset and refuses what is not RFC 3339.
#[test]
fn every_instant_field_normalizes_an_offset_and_refuses_the_rest() {
    let mut driven = 0_usize;
    for entry in declared() {
        for name in fields_with_format(&entry, "date-time") {
            for sample in driven_samples(&entry, &name) {
                let mut value = sample.clone();
                value[&name] = Value::String("2026-03-01T13:00:00+01:00".to_owned());
                let round = entry.round_trip(value).unwrap_or_else(|error| {
                    panic!("{}.{name} refused an offset: {error}", entry.name)
                });
                assert_eq!(
                    round.get(&name).and_then(Value::as_str),
                    Some("2026-03-01T12:00:00Z"),
                    "{}.{name} did not re-emit at a zero offset",
                    entry.name
                );

                for refused in ["2026-03-01T12:00:00", "the first of March"] {
                    let mut value = sample.clone();
                    value[&name] = Value::String(refused.to_owned());
                    assert!(
                        entry.round_trip(value).is_err(),
                        "{}.{name} accepted {refused}",
                        entry.name
                    );
                }
                driven += 1;
            }
        }
    }
    assert!(driven > 0, "no instant field was driven at all");
}

/// A value of no declared type is refused by every declared type.
#[test]
fn nothing_parses_a_value_of_no_declared_type() {
    let nonsense = serde_json::json!({ "printobserver": "no type declares this field" });
    for entry in declared() {
        assert!(
            entry.round_trip(nonsense.clone()).is_err(),
            "{} accepted a value of no declared type",
            entry.name
        );
    }
}
