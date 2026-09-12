//! The event kinds this adapter declares emit their schemas, marked with their kinds.
//!
//! The `printobserver-types:schemas` graph target runs this beside the other
//! declaring crates' `schemas` tests: with `PRINTOBSERVER_SCHEMAS=write` it
//! writes every kind's schema under `schemas/printobserver-obico/`, and without
//! it refuses a tree whose checked-in schema no longer matches what the types
//! generate. The marker each carries is written from the payload type's own
//! `KIND`, so a kind name is spelled once, on the type that owns it.

#[path = "support/schema_files.rs"]
mod schema_files;

use printobserver_obico::{
    OBICO_SOURCE, ObicoFailureAlertPayload, ObicoNotificationType, ObicoPrinterNotificationPayload,
    obico_source,
};
use printobserver_types::contract::{Sample as _, schema_of};
use printobserver_types::serde_json::Value;
use printobserver_types::{
    EVENT_KIND_MARKER, EventBody, EventPayload, WireField, event_schema_of, wire_fields,
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
fn generated() -> Vec<(String, Value)> {
    vec![
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
    ]
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
