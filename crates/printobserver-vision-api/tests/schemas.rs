//! This port's shapes and its one event kind carry the stated fields and emit
//! their schemas.

#[path = "support/schema_files.rs"]
mod schema_files;

use printobserver_types::contract::{Sample as _, schema_of};
use printobserver_types::serde_json::{self, Value};
use printobserver_types::{
    EVENT_KIND_MARKER, EventBody, EventPayload, WireField, event_schema_of, wire_fields,
};
use printobserver_vision_api::{
    Detection, FetchedImage, MalformedExternalEventPayload, NormalizedAlert, PrinterSignal,
    ProviderPrint,
};
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
fn generated() -> Vec<(String, printobserver_types::serde_json::Value)> {
    vec![
        (
            "NormalizedAlert.json".to_owned(),
            schema_of::<NormalizedAlert>(),
        ),
        ("FetchedImage.json".to_owned(), schema_of::<FetchedImage>()),
        (
            "ProviderPrint.json".to_owned(),
            schema_of::<ProviderPrint>(),
        ),
        ("Detection.json".to_owned(), schema_of::<Detection>()),
        (
            "PrinterSignal.json".to_owned(),
            schema_of::<PrinterSignal>(),
        ),
        (
            "MalformedExternalEventPayload.json".to_owned(),
            event_schema_of::<MalformedExternalEventPayload>(),
        ),
    ]
}

/// The checked-in schemas of this crate are what its types generate.
#[test]
fn the_checked_in_schemas_are_what_the_types_generate() {
    let findings = reconcile("printobserver-vision-api", &generated());
    assert!(
        findings.is_empty(),
        "the checked-in schemas have drifted:\n{}",
        findings.join("\n")
    );
}

/// `NormalizedAlert` carries exactly the fields the contract states.
#[test]
fn normalized_alert_carries_exactly_the_stated_fields() {
    let expected = vec![
        field("detection", "Detection", false),
        field("image_url", "string", false),
        field("kind", "EventKind", true),
        field("payload", "any", true),
        field("print", "ProviderPrint", false),
        field("raw", "RawBytes", true),
        field("received_at", "Timestamp", true),
        field("signal", "PrinterSignal", false),
        field("source", "EventSource", true),
    ];
    assert_eq!(wire_fields(&schema_of::<NormalizedAlert>()), expected);
}

/// `Detection` carries exactly the fields the contract states.
#[test]
fn detection_carries_exactly_the_stated_fields() {
    let expected = vec![
        field("paused_the_print", "boolean", true),
        field("provider_printer_id", "integer", true),
        field("warning", "boolean", true),
    ];
    assert_eq!(wire_fields(&schema_of::<Detection>()), expected);
}

/// `ProviderPrint` carries exactly the fields the contract states.
#[test]
fn provider_print_carries_exactly_the_stated_fields() {
    let expected = vec![
        field("file_name", "string", false),
        field("id", "integer", true),
        field("started_at", "Timestamp", false),
    ];
    assert_eq!(wire_fields(&schema_of::<ProviderPrint>()), expected);
}

/// The malformed kind is written under its own name, carries the stated field,
/// and reads back under that name and no other.
#[test]
fn the_malformed_kind_is_written_under_its_own_name_and_reads_back_under_it() {
    assert_eq!(
        MalformedExternalEventPayload::KIND,
        "malformed_external_event"
    );
    assert_eq!(
        wire_fields(&schema_of::<MalformedExternalEventPayload>()),
        vec![field("detail", "string", true)]
    );
    assert_eq!(
        event_schema_of::<MalformedExternalEventPayload>()[EVENT_KIND_MARKER],
        "malformed_external_event"
    );
    let written = MalformedExternalEventPayload::sample_full();
    let body = EventBody::of(&written).expect("a payload renders");
    assert_eq!(body.kind.as_str(), "malformed_external_event");
    assert_eq!(
        body.read::<MalformedExternalEventPayload>()
            .expect("under its own kind")
            .expect("of its own type"),
        written
    );
    let other = EventBody {
        kind: printobserver_types::EventKind::new("another_kind").expect("a kind name"),
        payload: body.payload.clone(),
    };
    assert!(other.read::<MalformedExternalEventPayload>().is_none());
}

/// `FetchedImage` carries exactly the fields the contract states.
#[test]
fn fetched_image_carries_exactly_the_stated_fields() {
    let expected = vec![
        field("bytes", "RawBytes", true),
        field("content_type", "string", true),
    ];
    assert_eq!(wire_fields(&schema_of::<FetchedImage>()), expected);
}

/// An alert as it was stored before alerts could carry a printer's signal.
const STORED_ALERT: &str = concat!(
    r#"{"source":"obico","received_at":"2026-03-01T12:00:00Z","#,
    r#""kind":"malformed_external_event","payload":{"detail":"unreadable"},"#,
    r#""raw":"e30=","image_url":"https://provider.example/1.jpg","#,
    r#""print":{"id":4211,"file_name":"benchy.gcode"},"#,
    r#""detection":{"warning":false,"paused_the_print":true,"provider_printer_id":41}}"#
);

/// An alert stored before `signal` existed reads as carrying none, and writes
/// back byte for byte as it was stored.
#[test]
fn an_alert_stored_without_a_signal_reads_as_none_and_writes_back_unchanged() {
    let alert: NormalizedAlert = serde_json::from_str(STORED_ALERT).expect("a stored alert reads");
    assert_eq!(alert.signal, None);
    assert_eq!(
        serde_json::to_string(&alert).expect("an alert writes"),
        STORED_ALERT
    );
}

/// A filament-change signal is written as its snake-case name, round-trips,
/// and is what the generated schema admits; a signal it does not name is
/// refused by the type and the schema alike.
#[test]
fn a_filament_change_signal_round_trips_and_validates() {
    let mut value: Value = serde_json::from_str(STORED_ALERT).expect("a stored alert reads");
    value["signal"] = Value::String("filament_change".to_owned());
    let alert: NormalizedAlert = serde_json::from_value(value.clone()).expect("a signal reads");
    assert_eq!(alert.signal, Some(PrinterSignal::FilamentChange));
    assert_eq!(
        serde_json::to_value(&alert).expect("an alert writes"),
        value
    );
    let validator = jsonschema::validator_for(&schema_of::<NormalizedAlert>())
        .expect("the generated schema compiles");
    assert!(validator.is_valid(&value), "the schema refused {value}");
    let signal = jsonschema::validator_for(&schema_of::<PrinterSignal>())
        .expect("the generated schema compiles");
    assert!(signal.is_valid(&Value::String("filament_change".to_owned())));
    assert!(!signal.is_valid(&Value::String("run_out".to_owned())));
    assert!(serde_json::from_value::<PrinterSignal>(Value::String("run_out".to_owned())).is_err());
}
