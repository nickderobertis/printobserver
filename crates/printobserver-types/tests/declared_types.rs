//! Every type this crate declares carries exactly the stated fields.
//!
//! The table below is the contract, written out: for each type this crate
//! declares as its own, the fields it carries, what each one is, and whether an
//! instance must carry it. The actual side is read off the schema each type
//! generates, so a type that gains a field, loses one, or changes one's type or
//! optionality is refused here rather than discovered by a consumer.
//!
//! A field's descriptor is the name of the type it references, or the JSON type
//! of a primitive; `array:T` and `map:T` name what a list or a map holds,
//! `union` is a field a tagged union declares at differing types in different
//! arms, and `any` is a field that admits any JSON value — the event envelope's
//! payload, whose form is its kind's own. A type whose whole wire form is a scalar — an identifier, an instant,
//! a file name — declares no field at all, and its wire form is asserted where
//! the wire forms are.

use printobserver_types::contract::declared;
use printobserver_types::{WireField, wire_fields};

/// One field of the contract table: its name, what it is, and whether an
/// instance must carry it.
type StatedField = (&'static str, &'static str, bool);

/// One type of the contract table: its name and the fields it carries.
type StatedType = (&'static str, &'static [StatedField]);

/// The fields this crate declares, type by type.
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
    ("Adjustable", &[]),
    ("EffectiveBounds", &[("allowed", "map:Range", true)]),
    ("EventId", &[]),
    ("EventKind", &[]),
    (
        "EventBody",
        &[("kind", "EventKind", true), ("payload", "any", true)],
    ),
    (
        "EventRecord",
        &[
            ("id", "EventId", true),
            ("image", "ImageRef", false),
            ("kind", "EventKind", true),
            ("payload", "any", true),
            ("print_id", "PrintId", false),
            ("raw", "RawBytes", false),
            ("received_at", "Timestamp", true),
            ("source", "EventSource", true),
        ],
    ),
    ("EventSource", &[]),
    ("ExecutionOutcome", &[("failed", "object", true)]),
    ("FileName", &[]),
    ("ImageId", &[]),
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
        "ImageRef",
        &[("id", "ImageId", true), ("sha256", "string", true)],
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
    ("PrintId", &[]),
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
    ("PrinterState", &[("unknown", "string", true)]),
    ("Range", &[("max", "number", true), ("min", "number", true)]),
    ("RawBytes", &[]),
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
        "Reported",
        &[("out_of_range", "boolean", true), ("value", "number", true)],
    ),
    (
        "SafetyEnvelope",
        &[
            ("actions", "map:array:ActionKind", true),
            ("agent_min_interval_s", "integer", true),
            ("allowed", "map:Range", true),
        ],
    ),
    ("SessionPhase", &[]),
    (
        "SupervisionSession",
        &[
            ("close_reason", "string", false),
            ("closed_at", "Timestamp", false),
            ("created_at", "Timestamp", true),
            ("harness_identity", "string", true),
            ("last_turn_at", "Timestamp", true),
            ("print_id", "PrintId", true),
            ("session_name", "string", true),
        ],
    ),
    ("Timestamp", &[]),
];

/// Every declared type carries exactly the fields the contract states.
#[test]
fn every_declared_type_carries_exactly_the_stated_fields() {
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

/// The contract table names no type this crate does not declare.
#[test]
fn the_contract_table_names_no_type_this_crate_does_not_declare() {
    let declared: Vec<&str> = declared().into_iter().map(|entry| entry.name).collect();
    for (name, _) in DECLARED_FIELDS {
        assert!(
            declared.contains(name),
            "the table names {name}, which this crate does not declare"
        );
    }
    assert_eq!(DECLARED_FIELDS.len(), declared.len());
}

/// Each declared type's registered name is the name its schema carries.
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
