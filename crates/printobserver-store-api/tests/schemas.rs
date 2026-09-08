//! This port's two shapes carry the stated fields and emit their schemas.

#[path = "support/schema_files.rs"]
mod schema_files;

use printobserver_store_api::{EventDraft, HistoryQuery};
use printobserver_types::contract::schema_of;
use printobserver_types::{WireField, wire_fields};
use schema_files::reconcile;

/// The field the contract states, as a name, what it is, and whether it is
/// required.
fn field(name: &str, descriptor: &str, required: bool) -> WireField {
    WireField { name: name.to_owned(), descriptor: descriptor.to_owned(), required }
}

/// The schemas this crate declares, by the file name each is written under.
fn generated() -> Vec<(String, printobserver_types::serde_json::Value)> {
    vec![
        ("EventDraft.json".to_owned(), schema_of::<EventDraft>()),
        ("HistoryQuery.json".to_owned(), schema_of::<HistoryQuery>()),
    ]
}

/// The checked-in schemas of this crate are what its types generate.
#[test]
fn the_checked_in_schemas_are_what_the_types_generate() {
    let findings = reconcile("printobserver-store-api", &generated());
    assert!(findings.is_empty(), "the checked-in schemas have drifted:\n{}", findings.join("\n"));
}

/// `EventDraft` carries exactly the fields the contract states.
#[test]
fn event_draft_carries_exactly_the_stated_fields() {
    let expected = vec![
        field("kind", "string", true),
        field("payload", "union", true),
        field("print_id", "PrintId", false),
        field("raw", "RawBytes", false),
        field("received_at", "Timestamp", true),
        field("source", "EventSource", true),
    ];
    assert_eq!(wire_fields(&schema_of::<EventDraft>()), expected);
}

/// `HistoryQuery` carries exactly the fields the contract states.
#[test]
fn history_query_carries_exactly_the_stated_fields() {
    let expected = vec![
        field("kinds", "array:EventKind", true),
        field("limit", "integer", false),
        field("print_id", "PrintId", true),
        field("since", "Timestamp", false),
        field("until", "Timestamp", false),
    ];
    assert_eq!(wire_fields(&schema_of::<HistoryQuery>()), expected);
}
