//! This port's two shapes carry the stated fields and emit their schemas.

#[path = "support/schema_files.rs"]
mod schema_files;

use printobserver_types::contract::schema_of;
use printobserver_types::{WireField, wire_fields};
use printobserver_vision_api::{FetchedImage, NormalizedAlert};
use schema_files::reconcile;

/// The field the contract states, as a name, what it is, and whether it is
/// required.
fn field(name: &str, descriptor: &str, required: bool) -> WireField {
    WireField { name: name.to_owned(), descriptor: descriptor.to_owned(), required }
}

/// The schemas this crate declares, by the file name each is written under.
fn generated() -> Vec<(String, printobserver_types::serde_json::Value)> {
    vec![
        ("NormalizedAlert.json".to_owned(), schema_of::<NormalizedAlert>()),
        ("FetchedImage.json".to_owned(), schema_of::<FetchedImage>()),
    ]
}

/// The checked-in schemas of this crate are what its types generate.
#[test]
fn the_checked_in_schemas_are_what_the_types_generate() {
    let findings = reconcile("printobserver-vision-api", &generated());
    assert!(findings.is_empty(), "the checked-in schemas have drifted:\n{}", findings.join("\n"));
}

/// `NormalizedAlert` carries exactly the fields the contract states.
#[test]
fn normalized_alert_carries_exactly_the_stated_fields() {
    let expected = vec![
        field("image_url", "string", false),
        field("kind", "string", true),
        field("payload", "union", true),
        field("raw", "RawBytes", true),
        field("received_at", "Timestamp", true),
        field("source", "EventSource", true),
    ];
    assert_eq!(wire_fields(&schema_of::<NormalizedAlert>()), expected);
}

/// `FetchedImage` carries exactly the fields the contract states.
#[test]
fn fetched_image_carries_exactly_the_stated_fields() {
    let expected =
        vec![field("bytes", "RawBytes", true), field("content_type", "string", true)];
    assert_eq!(wire_fields(&schema_of::<FetchedImage>()), expected);
}
