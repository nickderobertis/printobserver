//! The supervision domain's records carry the stated fields and the stated
//! wire forms.
//!
//! The table below is the contract, written out: for each record this domain
//! declares, the fields it carries, what each one is, and whether an instance
//! must carry it. The actual side is read off the schema each type generates,
//! so a record that gains a field, loses one, or changes one's type or
//! optionality is refused here rather than discovered by a consumer. A field's
//! descriptor is the name of the type it references, or the JSON type of a
//! primitive; `array:T` and `map:T` name what a list or a map holds, `union` is
//! a field a tagged union declares at differing types in different arms. A
//! type whose whole wire form is a scalar — the two identifiers this domain
//! mints — declares no field at all, and its wire form is asserted where the
//! wire forms are.
//!
//! The wire forms are asserted on the *serialized form* rather than on the
//! value that survives a round trip: a representation that survives a round
//! trip while serializing in another form would satisfy the first and fail the
//! second. Beside the corpus of well-formed values stands an adversarial walk,
//! because a corpus establishes nothing about values it does not contain. The
//! walk is anchored to the declared records rather than to a sample: it reads
//! which fields are identifiers and which are instants off the schema each
//! type generates, so a record that gains a field of either kind is driven
//! without anyone remembering to add it.
//!
//! The accessors a consumer reads off a record — an action's kind, an actor's
//! class — are held here too, to agreeing with the data they read; and so are
//! the two identifiers this domain mints through the type crate's exported
//! rule, which are held to minting, displaying and refusing exactly as the
//! three the envelope reaches do.

#[path = "support/records.rs"]
mod records;

use std::collections::BTreeMap;
use std::str::FromStr as _;

use printobserver_core::{
    AcknowledgementDisposition, ActionId, ActionKind, Actor, ActorClass, EffectiveBounds,
    InterventionId, PrintAction, SafetyEnvelope,
};
use printobserver_types::contract::{Sample, TypeContract};
use printobserver_types::serde_json::{self, Value};
use printobserver_types::{Adjustable, Range, WireField, wire_fields};
use records::declared;

/// One field of the contract table: its name, what it is, and whether an
/// instance must carry it.
type StatedField = (&'static str, &'static str, bool);

/// One type of the contract table: its name and the fields it carries.
type StatedType = (&'static str, &'static [StatedField]);

/// The fields this domain's records carry, type by type.
const DECLARED_FIELDS: &[StatedType] = &[
    ("AcknowledgementDisposition", &[]),
    ("ActionId", &[]),
    ("ActionKind", &[]),
    (
        "ActionRecord",
        &[
            ("decision", "PolicyDecision", true),
            ("executed_at", "Timestamp", false),
            ("id", "ActionId", true),
            ("outcome", "ExecutionOutcome", false),
            ("print_id", "PrintId", true),
            ("request", "ActionRequest", true),
        ],
    ),
    (
        "ActionRequest",
        &[
            ("action", "PrintAction", true),
            ("actor", "Actor", true),
            ("requested_at", "Timestamp", true),
        ],
    ),
    ("Actor", &[("agent", "object", true)]),
    ("ActorClass", &[]),
    ("EffectiveBounds", &[("allowed", "map:Range", true)]),
    ("ExecutionOutcome", &[("failed", "object", true)]),
    (
        "ImageRecord",
        &[
            ("byte_len", "integer", true),
            ("content_type", "string", true),
            ("event_id", "EventId", true),
            ("fetched_at", "Timestamp", true),
            ("id", "ImageId", true),
            ("print_id", "PrintId", true),
            ("relative_path", "string", true),
            ("sha256", "string", true),
            ("source_url", "string", false),
        ],
    ),
    (
        "Intervention",
        &[
            ("action_id", "ActionId", true),
            ("adjustable", "Adjustable", true),
            ("applied_at", "Timestamp", true),
            ("applied_value", "number", true),
            ("expires_at", "Timestamp", true),
            ("id", "InterventionId", true),
            ("outcome", "InterventionOutcome", true),
            ("print_id", "PrintId", true),
            ("prior_value", "number", false),
            ("restored_at", "Timestamp", false),
        ],
    ),
    ("InterventionId", &[]),
    (
        "InterventionOutcome",
        &[
            ("restore_failed", "object", true),
            ("superseded", "object", true),
        ],
    ),
    (
        "JobManifest",
        &[
            ("allowed", "map:Range", true),
            ("file_name", "string", true),
            ("material", "string", true),
            ("metadata", "map:string", true),
            ("nozzle_diameter_mm", "number", true),
            ("slicer_profile", "string", true),
        ],
    ),
    (
        "ManifestNarrowing",
        &[
            ("adjustable", "Adjustable", true),
            ("applied", "Range", true),
            ("requested", "Range", true),
        ],
    ),
    ("PolicyDecision", &[("rejected", "RejectionReason", true)]),
    (
        "PrintAction",
        &[
            ("action", "string", true),
            ("actor", "Actor", true),
            ("disposition", "AcknowledgementDisposition", true),
            ("duration_s", "integer", false),
            ("event_id", "EventId", true),
            ("factor", "number", true),
            ("file_name", "FileName", true),
            ("manifest", "JobManifest", true),
            ("percent", "number", true),
            ("reason", "string", true),
            ("target_c", "number", true),
            ("tool", "integer", true),
        ],
    ),
    (
        "PrintRecord",
        &[
            ("end_reason", "string", false),
            ("ended_at", "Timestamp", false),
            ("file_name", "string", false),
            ("id", "PrintId", true),
            ("narrowings", "array:ManifestNarrowing", true),
            ("obico_print_id", "integer", false),
            ("opened_at", "Timestamp", true),
            ("state", "PrinterState", true),
        ],
    ),
    (
        "RejectionReason",
        &[
            ("actor_may_not_request", "object", true),
            ("invalid_from_state", "object", true),
            ("min_interval_not_elapsed", "object", true),
            ("out_of_bounds", "object", true),
            ("unsupported_adjustable", "object", true),
        ],
    ),
    (
        "SafetyEnvelope",
        &[
            ("actions", "map:array:ActionKind", true),
            ("agent_min_interval_s", "integer", true),
            ("allowed", "map:Range", true),
        ],
    ),
];

/// Every declared record carries exactly the fields the contract states.
#[test]
fn every_declared_record_carries_exactly_the_stated_fields() {
    for entry in declared() {
        let expected = DECLARED_FIELDS
            .iter()
            .find(|(name, _)| *name == entry.name)
            .unwrap_or_else(|| panic!("{} is declared but the contract table omits it", entry.name))
            .1;
        let expected: Vec<WireField> = expected
            .iter()
            .map(|(name, descriptor, required)| WireField {
                name: (*name).to_owned(),
                descriptor: (*descriptor).to_owned(),
                required: *required,
            })
            .collect();
        assert_eq!(
            wire_fields(&entry.schema()),
            expected,
            "{} does not carry the stated fields",
            entry.name
        );
    }
}

/// The contract table names no record this domain does not declare.
#[test]
fn the_contract_table_names_no_record_this_domain_does_not_declare() {
    let declared: Vec<&str> = declared().into_iter().map(|entry| entry.name).collect();
    for (name, _) in DECLARED_FIELDS {
        assert!(
            declared.contains(name),
            "the table names {name}, which this domain does not declare"
        );
    }
    assert_eq!(DECLARED_FIELDS.len(), declared.len());
}

/// Each declared record's registered name is the name its schema carries.
#[test]
fn each_declared_name_is_the_name_its_schema_carries() {
    for entry in declared() {
        let schema = entry.schema();
        let title = schema.get("title").and_then(|value| value.as_str());
        assert_eq!(
            title,
            Some(entry.name),
            "{} generates a schema titled {title:?}",
            entry.name
        );
    }
}

/// Every action reads back the kind, the reason and the actor it carries.
#[test]
fn every_action_reads_back_what_it_carries() {
    let mut kinds = Vec::new();
    for action in
        std::iter::once(PrintAction::sample_full()).chain(PrintAction::sample_alternates())
    {
        assert!(
            !action.reason().is_empty(),
            "{action:?} carries an empty reason"
        );
        assert_eq!(action.actor().class(), action.actor().class());
        kinds.push(action.kind());
    }
    kinds.sort();
    kinds.dedup();
    assert_eq!(kinds.len(), 10, "the corpus does not carry every kind");
    assert_eq!(
        PrintAction::sample_full().kind(),
        ActionKind::SetFeedrateFactor
    );
}

/// Every actor reports the class a safety envelope grants actions to.
#[test]
fn every_actor_reports_its_class() {
    assert_eq!(
        Actor::Agent {
            session_name: "print-1".to_owned()
        }
        .class(),
        ActorClass::Agent
    );
    assert_eq!(Actor::Operator.class(), ActorClass::Operator);
    assert_eq!(Actor::System.class(), ActorClass::System);
}

/// The disposition an acknowledgement samples as is the one the corpus names.
#[test]
fn the_disposition_samples_as_watch() {
    assert_eq!(
        AcknowledgementDisposition::sample_full(),
        AcknowledgementDisposition::Watch
    );
}

/// A minted identifier of this domain is a version 7 UUID, displayed in the
/// one spelling, and refused in every other — exactly as the type crate's own.
#[test]
fn a_minted_identifier_of_this_domain_is_a_version_seven_uuid() {
    let minted = ActionId::new();
    assert_eq!(minted.as_uuid().get_version_num(), 7);
    assert_eq!(minted.to_string().parse::<ActionId>(), Ok(minted));
    assert_eq!(InterventionId::default().as_uuid().get_version_num(), 7);
    assert_ne!(
        InterventionId::new(),
        InterventionId::new(),
        "two mintings are the same identifier"
    );

    let error = ActionId::from_str("nonsense").expect_err("the spelling is refused");
    assert_eq!(error.type_name(), "ActionId");
    assert!(error.detail().contains("nonsense"), "{}", error.detail());
    let error = InterventionId::from_str("0191F0A0-0000-7000-8000-000000000001")
        .expect_err("the uppercase spelling is refused");
    assert_eq!(error.type_name(), "InterventionId");
}

/// An envelope granting one member one range, and every other a distant one.
fn envelope(member: Adjustable, range: Range) -> SafetyEnvelope {
    let mut allowed = BTreeMap::from([
        (Adjustable::Feedrate, Range::new(-1_000.0, -999.0)),
        (Adjustable::Flowrate, Range::new(-1_000.0, -999.0)),
        (Adjustable::Fan, Range::new(-1_000.0, -999.0)),
        (Adjustable::BedTarget, Range::new(-1_000.0, -999.0)),
        (
            Adjustable::ToolTarget { tool: 0 },
            Range::new(-1_000.0, -999.0),
        ),
    ]);
    allowed.insert(member, range);
    SafetyEnvelope {
        allowed,
        actions: BTreeMap::from([(ActorClass::Agent, vec![ActionKind::Pause])]),
        agent_min_interval_s: 60,
    }
}

/// The safety envelope is a separate contract from the printer port's
/// plausibility ranges: an envelope inside a plausibility range and one
/// outside it are both representable and both survive a round trip exactly as
/// given, for each of the five adjustables the port ranges.
#[test]
fn an_envelope_inside_or_outside_a_plausibility_range_is_carried_exactly() {
    let inside_and_outside: [(Adjustable, Range, Range); 5] = [
        (
            Adjustable::Feedrate,
            Range::new(0.5, 1.5),
            Range::new(20.0, 40.0),
        ),
        (
            Adjustable::Flowrate,
            Range::new(0.9, 1.1),
            Range::new(-5.0, -1.0),
        ),
        (
            Adjustable::Fan,
            Range::new(20.0, 80.0),
            Range::new(200.0, 400.0),
        ),
        (
            Adjustable::ToolTarget { tool: 0 },
            Range::new(190.0, 230.0),
            Range::new(900.0, 1_200.0),
        ),
        (
            Adjustable::BedTarget,
            Range::new(40.0, 70.0),
            Range::new(900.0, 1_200.0),
        ),
    ];
    for (member, inside, outside) in inside_and_outside {
        for range in [inside, outside] {
            let held = envelope(member, range);
            let round: SafetyEnvelope =
                serde_json::from_value(serde_json::to_value(&held).expect("an envelope emits"))
                    .expect("an envelope parses");
            assert_eq!(
                round.allowed.get(&member),
                Some(&range),
                "{member} was not carried exactly"
            );

            let bounds = EffectiveBounds {
                allowed: BTreeMap::from([(member, range)]),
            };
            let round: EffectiveBounds =
                serde_json::from_value(serde_json::to_value(&bounds).expect("bounds emit"))
                    .expect("bounds parse");
            assert_eq!(
                round.allowed.get(&member),
                Some(&range),
                "{member} was not carried exactly"
            );
        }
    }
}

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
