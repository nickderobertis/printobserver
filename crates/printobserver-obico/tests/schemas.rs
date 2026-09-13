//! The types this adapter declares emit their schemas, and the event kinds
//! among them are marked with their kinds.
//!
//! The `printobserver-types:schemas` graph target runs this beside the other
//! declaring crates' `schemas` tests: with `PRINTOBSERVER_SCHEMAS=write` it
//! writes every schema under `schemas/printobserver-obico/`, and without it
//! refuses a tree whose checked-in schema no longer matches what the types
//! generate. The marker each kind carries is written from the payload type's
//! own `KIND`, so a kind name is spelled once, on the type that owns it.
//!
//! The nine wire shapes the producer sends are this crate's declarations too,
//! so the table of the fields each carries, the round trip of every canonical
//! value and the absent-optional rule are walked here over them.

#[path = "support/schema_files.rs"]
mod schema_files;

use printobserver_obico::{
    OBICO_SOURCE, ObicoEventType, ObicoFailureAlert, ObicoFailureAlertPayload, ObicoFailureEvent,
    ObicoFailureEventType, ObicoNotificationEvent, ObicoNotificationType, ObicoPrintInfo,
    ObicoPrinterInfo, ObicoPrinterNotification, ObicoPrinterNotificationPayload, ObicoTimestamp,
    obico_source,
};
use printobserver_types::contract::{Sample as _, TypeContract, schema_of};
use printobserver_types::serde_json::Value;
use printobserver_types::{
    EVENT_KIND_MARKER, EventBody, EventPayload, WireField, event_schema_of, wire_fields,
};
use schema_files::reconcile;

/// The nine wire shapes this crate declares, each with its canonical values.
fn wire_shapes() -> Vec<TypeContract> {
    vec![
        TypeContract::of::<ObicoEventType>("ObicoEventType"),
        TypeContract::of::<ObicoFailureAlert>("ObicoFailureAlert"),
        TypeContract::of::<ObicoFailureEvent>("ObicoFailureEvent"),
        TypeContract::of::<ObicoFailureEventType>("ObicoFailureEventType"),
        TypeContract::of::<ObicoNotificationEvent>("ObicoNotificationEvent"),
        TypeContract::of::<ObicoPrintInfo>("ObicoPrintInfo"),
        TypeContract::of::<ObicoPrinterInfo>("ObicoPrinterInfo"),
        TypeContract::of::<ObicoPrinterNotification>("ObicoPrinterNotification"),
        TypeContract::of::<ObicoTimestamp>("ObicoTimestamp"),
    ]
}

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
    let mut entries = vec![
        (
            "ObicoFailureAlertPayload.json".to_owned(),
            event_schema_of::<ObicoFailureAlertPayload>(),
        ),
        (
            "ObicoPrinterNotificationPayload.json".to_owned(),
            event_schema_of::<ObicoPrinterNotificationPayload>(),
        ),
        (
            "ObicoNotificationType.json".to_owned(),
            schema_of::<ObicoNotificationType>(),
        ),
    ];
    entries.extend(
        wire_shapes()
            .into_iter()
            .map(|entry| (format!("{}.json", entry.name), entry.schema())),
    );
    entries
}

/// The checked-in schemas of this crate are what its types generate.
#[test]
fn the_checked_in_schemas_are_what_the_types_generate() {
    let findings = reconcile("printobserver-obico", &generated());
    assert!(
        findings.is_empty(),
        "the checked-in schemas have drifted:\n{}",
        findings.join("\n")
    );
}

/// Each kind is written under the name the history has always carried, and
/// its schema's marker is that name.
#[test]
fn each_kind_is_written_under_its_own_name() {
    assert_eq!(ObicoFailureAlertPayload::KIND, "obico_failure_alert");
    assert_eq!(
        ObicoPrinterNotificationPayload::KIND,
        "obico_printer_notification"
    );
    assert_eq!(
        event_schema_of::<ObicoFailureAlertPayload>()[EVENT_KIND_MARKER],
        "obico_failure_alert"
    );
    assert_eq!(
        event_schema_of::<ObicoPrinterNotificationPayload>()[EVENT_KIND_MARKER],
        "obico_printer_notification"
    );
    assert_eq!(OBICO_SOURCE, "obico");
    assert_eq!(obico_source().as_str(), "obico");
}

/// The notification type is a closed set of eight spellings, and its sample is
/// one of them.
#[test]
fn the_notification_type_is_a_closed_set_of_eight() {
    let schema = schema_of::<ObicoNotificationType>();
    let spellings: Vec<&str> = schema["oneOf"]
        .as_array()
        .expect("a closed set of arms")
        .iter()
        .filter_map(|arm| arm["const"].as_str())
        .collect();
    assert_eq!(
        spellings,
        [
            "started",
            "done",
            "cancelled",
            "paused",
            "resumed",
            "filament_change",
            "heater_cooled",
            "heater_target",
        ]
    );
    let sample = printobserver_types::serde_json::to_value(ObicoNotificationType::sample_full())
        .expect("a fieldless enum serializes");
    assert!(spellings.contains(&sample.as_str().expect("a spelling")));
}

/// The two payloads carry exactly the fields the contract states.
#[test]
fn the_payloads_carry_exactly_the_stated_fields() {
    assert_eq!(
        wire_fields(&schema_of::<ObicoFailureAlertPayload>()),
        vec![
            field("ended_at", "Timestamp", false),
            field("file_name", "string", false),
            field("is_warning", "boolean", true),
            field("obico_print_id", "integer", false),
            field("print_paused", "boolean", true),
            field("started_at", "Timestamp", false),
        ]
    );
    assert_eq!(
        wire_fields(&schema_of::<ObicoPrinterNotificationPayload>()),
        vec![
            field("ended_at", "Timestamp", false),
            field("file_name", "string", false),
            field("notification_type", "ObicoNotificationType", true),
            field("obico_print_id", "integer", false),
            field("started_at", "Timestamp", false),
        ]
    );
}

/// Every canonical value reads back under its own kind, and an absent optional
/// serializes as absent rather than as null.
#[test]
fn every_canonical_value_reads_back_under_its_own_kind() {
    for sample in [
        ObicoFailureAlertPayload::sample_full(),
        ObicoFailureAlertPayload::sample_minimal(),
    ] {
        let body = EventBody::of(&sample).expect("a payload renders");
        assert_eq!(body.kind.as_str(), "obico_failure_alert");
        assert_eq!(
            body.read::<ObicoFailureAlertPayload>()
                .expect("under its own kind")
                .expect("of its own type"),
            sample
        );
        assert!(body.read::<ObicoPrinterNotificationPayload>().is_none());
    }
    let minimal = EventBody::of(&ObicoPrinterNotificationPayload::sample_minimal())
        .expect("a payload renders");
    assert_eq!(minimal.kind.as_str(), "obico_printer_notification");
    let object = minimal.payload.as_object().expect("an object");
    assert!(
        !object.contains_key("obico_print_id") && !object.contains_key("file_name"),
        "an absent optional was serialized: {object:?}"
    );
    assert_eq!(
        minimal
            .read::<ObicoPrinterNotificationPayload>()
            .expect("under its own kind")
            .expect("of its own type"),
        ObicoPrinterNotificationPayload::sample_minimal()
    );
}

/// One field of the contract table: its name, what it is, and whether an
/// instance must carry it.
type StatedField = (&'static str, &'static str, bool);

/// The fields the nine wire shapes carry, shape by shape.
///
/// The actual side is read off the schema each type generates, so a shape that
/// gains a field, loses one, or changes one's type or optionality is refused
/// here rather than discovered by the scheduled tier against a live producer.
const WIRE_FIELDS: &[(&str, &[StatedField])] = &[
    ("ObicoEventType", &[]),
    (
        "ObicoFailureAlert",
        &[
            ("event", "ObicoFailureEvent", true),
            ("img_url", "string", true),
            ("print", "ObicoPrintInfo", true),
            ("printer", "ObicoPrinterInfo", true),
        ],
    ),
    (
        "ObicoFailureEvent",
        &[
            ("is_warning", "boolean", true),
            ("print_paused", "boolean", true),
            ("type", "ObicoFailureEventType", true),
        ],
    ),
    ("ObicoFailureEventType", &[]),
    (
        "ObicoNotificationEvent",
        &[
            ("is_warning", "boolean", true),
            ("print_paused", "boolean", true),
            ("type", "ObicoEventType", true),
        ],
    ),
    (
        "ObicoPrintInfo",
        &[
            ("ended_at", "ObicoTimestamp", false),
            ("filename", "string", true),
            ("id", "integer", true),
            ("started_at", "ObicoTimestamp", false),
        ],
    ),
    (
        "ObicoPrinterInfo",
        &[("id", "integer", true), ("name", "string", true)],
    ),
    (
        "ObicoPrinterNotification",
        &[
            ("event", "ObicoNotificationEvent", true),
            ("img_url", "string", false),
            ("print", "ObicoPrintInfo", false),
            ("printer", "ObicoPrinterInfo", true),
        ],
    ),
    ("ObicoTimestamp", &[]),
];

/// Every wire shape carries exactly the fields the contract states, and the
/// table names no shape this crate does not declare.
#[test]
fn every_wire_shape_carries_exactly_the_stated_fields() {
    let shapes = wire_shapes();
    assert_eq!(WIRE_FIELDS.len(), shapes.len());
    for entry in shapes {
        let expected = WIRE_FIELDS
            .iter()
            .find(|(name, _)| *name == entry.name)
            .unwrap_or_else(|| panic!("{} is declared but the table omits it", entry.name))
            .1;
        let expected: Vec<WireField> = expected
            .iter()
            .map(|(name, descriptor, required)| field(name, descriptor, *required))
            .collect();
        assert_eq!(
            wire_fields(&entry.schema()),
            expected,
            "{} does not carry the stated fields",
            entry.name
        );
        assert_eq!(
            entry.schema().get("title").and_then(Value::as_str),
            Some(entry.name),
            "{} generates a schema titled otherwise",
            entry.name
        );
    }
}

/// Every wire shape round-trips every one of its canonical values unchanged.
#[test]
fn every_wire_shape_round_trips_its_canonical_values() {
    for entry in wire_shapes() {
        for value in entry.samples() {
            let round = entry
                .round_trip(value.clone())
                .unwrap_or_else(|error| panic!("{}: {error}", entry.name));
            assert_eq!(round, value, "{} does not survive a round trip", entry.name);
        }
    }
}

/// An optional field the producer omits is absent from the serialized object,
/// never `null`: an absent instant is one the producer did not send.
#[test]
fn an_absent_optional_is_absent_from_the_serialized_object() {
    let mut checked = 0_usize;
    for entry in wire_shapes() {
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
    assert_eq!(checked, 4, "the four optional fields were not all checked");
}

/// A value of no declared shape is refused by every wire shape.
#[test]
fn nothing_parses_a_value_of_no_declared_shape() {
    let nonsense = printobserver_types::serde_json::json!({ "obico": "no shape declares this" });
    for entry in wire_shapes() {
        assert!(
            entry.round_trip(nonsense.clone()).is_err(),
            "{} accepted a value of no declared shape",
            entry.name
        );
    }
}
