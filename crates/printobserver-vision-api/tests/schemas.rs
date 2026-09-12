//! This port's shapes and its one event kind carry the stated fields and emit
//! their schemas.

#[path = "support/schema_files.rs"]
mod schema_files;

use printobserver_types::contract::{Sample as _, schema_of};
use printobserver_types::{
    EVENT_KIND_MARKER, EventBody, EventPayload, WireField, event_schema_of, wire_fields,
};
use printobserver_vision_api::{
    FetchedImage, MalformedExternalEventPayload, NormalizedAlert, ProviderPrint,
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
        field("image_url", "string", false),
        field("kind", "EventKind", true),
        field("payload", "any", true),
        field("print", "ProviderPrint", false),
        field("raw", "RawBytes", true),
        field("received_at", "Timestamp", true),
        field("source", "EventSource", true),
    ];
    assert_eq!(wire_fields(&schema_of::<NormalizedAlert>()), expected);
}

/// `ProviderPrint` carries exactly the fields the contract states.
#[test]
fn provider_print_carries_exactly_the_stated_fields() {
    let expected = vec![
        field("file_name", "string", false),
        field("id", "integer", true),
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
        kind: printobserver_types::EventKind::new("another_kind"),
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
