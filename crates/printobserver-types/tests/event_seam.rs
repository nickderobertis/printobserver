//! The event envelope is open, and a typed payload is read back only under its own kind.
//!
//! Nothing in this crate declares a kind, so the payload types here are this
//! test's own, declared under kinds no crate ships — which is exactly what a
//! domain does when it declares one. What is proven is the seam itself: a
//! payload read back under its own kind is what was written, nothing is read
//! back under another's, a record under a kind nothing declares survives a
//! round trip unchanged, the wire form is the eight fields and no other, and
//! the schema marker carries the kind.

use printobserver_types::contract::Sample as _;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};
use printobserver_types::{
    EVENT_KIND_MARKER, EventBody, EventKind, EventKindError, EventPayload, EventRecord,
    EventSource, KIND_PATTERN, event_schema_of, schema_of, wire_fields,
};
use serde_json::json;

/// A payload this test declares, under a kind no crate ships.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct Sighting {
    /// What was seen.
    what: String,
    /// How many times.
    count: i64,
}

impl EventPayload for Sighting {
    const KIND: &'static str = "test_sighting";
}

/// A second payload, under a second kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct Silence {
    /// For how long.
    seconds: i64,
}

impl EventPayload for Silence {
    const KIND: &'static str = "test_silence";
}

/// A record carrying one body, with every other field the sample's.
fn record(body: EventBody) -> EventRecord {
    EventRecord {
        body,
        ..EventRecord::sample_full()
    }
}

/// A payload read back under its own kind is what was written.
#[test]
fn a_payload_reads_back_under_its_own_kind() {
    let written = Sighting {
        what: "spaghetti".to_owned(),
        count: 3,
    };
    let body = EventBody::of(&written).expect("a payload renders");
    assert_eq!(
        body.kind,
        EventKind::new("test_sighting").expect("a kind name")
    );
    assert_eq!(body.kind, Sighting::kind());
    assert!(body.is::<Sighting>());
    let read = body
        .read::<Sighting>()
        .expect("the body is under this kind")
        .expect("the payload is of this type");
    assert_eq!(read, written);

    let held = record(body);
    assert_eq!(held.kind().as_str(), "test_sighting");
    let read = held
        .payload_as::<Sighting>()
        .expect("the record is under this kind")
        .expect("the payload is of this type");
    assert_eq!(read, written);
}

/// Nothing is read back under another kind, and a kind is compared by name.
#[test]
fn nothing_reads_back_under_another_kind() {
    let body = EventBody::of(&Silence { seconds: 40 }).expect("a payload renders");
    assert!(!body.is::<Sighting>());
    assert!(body.read::<Sighting>().is_none());
    assert!(record(body).payload_as::<Sighting>().is_none());
}

/// A body under the right kind whose payload is not of the type is a parse
/// failure rather than nothing, so a producer that wrote the wrong shape is
/// heard from.
#[test]
fn a_payload_of_the_wrong_shape_under_the_right_kind_is_a_parse_failure() {
    let body = EventBody {
        kind: Sighting::kind(),
        payload: json!({ "what": "spaghetti" }),
    };
    let answered = body
        .read::<Sighting>()
        .expect("the body is under this kind");
    assert!(answered.is_err(), "a payload missing a field parsed");
}

/// A record under a kind no crate declares survives a round trip unchanged.
#[test]
fn a_record_under_an_undeclared_kind_survives_a_round_trip() {
    let held = record(EventBody {
        kind: EventKind::new("kind_from_a_newer_server").expect("a kind name"),
        payload: json!({ "anything": [1, 2, 3], "nested": { "deeply": true } }),
    });
    let text = serde_json::to_string(&held).expect("a record serializes");
    let read: EventRecord = serde_json::from_str(&text).expect("a record parses");
    assert_eq!(read, held);
    assert_eq!(read.kind().as_str(), "kind_from_a_newer_server");
    assert!(read.payload_as::<Sighting>().is_none());
    assert!(read.payload_as::<Silence>().is_none());
}

/// The wire form of a record is the eight fields and nothing else, with the
/// kind and the payload beside the rest rather than nested under a body.
#[test]
fn the_wire_form_is_the_eight_fields_and_nothing_else() {
    let held = record(
        EventBody::of(&Sighting {
            what: "spaghetti".to_owned(),
            count: 3,
        })
        .expect("a payload renders"),
    );
    let value = serde_json::to_value(&held).expect("a record serializes");
    let object = value.as_object().expect("a record is an object");
    let mut names: Vec<&str> = object.keys().map(String::as_str).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "id",
            "image",
            "kind",
            "payload",
            "print_id",
            "raw",
            "received_at",
            "source"
        ]
    );
    assert_eq!(value["kind"], json!("test_sighting"));
    assert_eq!(value["payload"], json!({ "what": "spaghetti", "count": 3 }));
    assert_eq!(value["source"], json!("sample_source"));

    let declared: Vec<(String, bool)> = wire_fields(&schema_of::<EventRecord>())
        .into_iter()
        .map(|field| (field.name, field.required))
        .collect();
    assert_eq!(
        declared,
        [
            ("id".to_owned(), true),
            ("image".to_owned(), false),
            ("kind".to_owned(), true),
            ("payload".to_owned(), true),
            ("print_id".to_owned(), false),
            ("raw".to_owned(), false),
            ("received_at".to_owned(), true),
            ("source".to_owned(), true),
        ]
    );
}

/// The pair's own wire form is `kind` beside `payload`, and a kind and a
/// source each serialize as the bare string.
#[test]
fn the_pair_and_the_names_serialize_bare() {
    let body = EventBody::of(&Silence { seconds: 40 }).expect("a payload renders");
    assert_eq!(
        serde_json::to_value(&body).expect("a body serializes"),
        json!({ "kind": "test_silence", "payload": { "seconds": 40 } })
    );
    assert_eq!(
        serde_json::to_value(EventKind::new("test_silence").expect("a kind name"))
            .expect("a kind serializes"),
        json!("test_silence")
    );
    assert_eq!(
        serde_json::to_value(EventSource::new("obico")).expect("a source serializes"),
        json!("obico")
    );
    assert_eq!(
        EventKind::new("test_silence")
            .expect("a kind name")
            .to_string(),
        "test_silence"
    );
    assert_eq!(EventSource::new("obico").to_string(), "obico");
    let mut kinds = std::collections::BTreeSet::new();
    kinds.insert(EventKind::new("b").expect("a kind name"));
    kinds.insert(EventKind::new("a").expect("a kind name"));
    assert_eq!(
        kinds.iter().map(EventKind::as_str).collect::<Vec<_>>(),
        ["a", "b"]
    );
}

/// The schema of a payload is the type's own plus the marker naming its kind,
/// and the envelope's schema declares a kind as a string under the pattern.
#[test]
fn the_schema_marker_carries_the_kind() {
    let marked = event_schema_of::<Sighting>();
    assert_eq!(marked[EVENT_KIND_MARKER], json!("test_sighting"));
    let mut unmarked = marked.clone();
    unmarked
        .as_object_mut()
        .expect("an object")
        .remove(EVENT_KIND_MARKER);
    assert_eq!(unmarked, schema_of::<Sighting>());

    let kind = schema_of::<EventKind>();
    assert_eq!(kind["type"], json!("string"));
    assert_eq!(kind["pattern"], json!(KIND_PATTERN));
    assert_eq!(schema_of::<EventSource>()["type"], json!("string"));
    let envelope = schema_of::<EventRecord>();
    let payload = envelope["properties"]["payload"]
        .as_object()
        .expect("the payload's schema is an object");
    assert!(
        payload.keys().all(|key| key == "description"),
        "the payload constrains its form: {payload:?}"
    );
    assert_eq!(
        envelope["properties"]["kind"]["$ref"],
        json!("#/$defs/EventKind")
    );
}

/// The field reader says `any` for a payload of any form, in either spelling
/// `schemars` emits, and `union` for a field the arms of a schema declare at
/// differing types.
#[test]
fn the_field_reader_reads_any_and_union() {
    let schema = json!({
        "type": "object",
        "properties": {
            "bare": true,
            "documented": { "description": "a value of any form" },
            "named": { "$ref": "#/$defs/EventKind" }
        },
        "required": ["bare", "documented"],
        "oneOf": [
            { "properties": { "arm": { "type": "string" } }, "required": ["arm"] },
            { "properties": { "arm": { "type": "integer" } } }
        ]
    });
    let fields: Vec<(String, String, bool)> = wire_fields(&schema)
        .into_iter()
        .map(|field| (field.name, field.descriptor, field.required))
        .collect();
    assert_eq!(
        fields,
        [
            ("arm".to_owned(), "union".to_owned(), false),
            ("bare".to_owned(), "any".to_owned(), true),
            ("documented".to_owned(), "any".to_owned(), true),
            ("named".to_owned(), "EventKind".to_owned(), false),
        ]
    );
}

/// A kind name is lowercase `snake_case` and nothing else, on construction and
/// on parse alike: the pattern the schema declares is the one the type holds.
#[test]
fn a_kind_name_outside_the_pattern_is_refused() {
    for admitted in ["a", "obico_failure_alert", "kind_2", "x_"] {
        assert_eq!(
            EventKind::new(admitted).map(|kind| kind.as_str().to_owned()),
            Ok(admitted.to_owned())
        );
        assert_eq!(admitted.parse::<EventKind>(), EventKind::new(admitted));
    }
    for refused in [
        "",
        "Kind",
        "kind-name",
        "kind name",
        "_kind",
        "1kind",
        "kind.name",
        "kÿnd",
    ] {
        let error = EventKind::new(refused).expect_err(refused);
        assert_eq!(error.name(), refused);
        assert!(
            error.to_string().contains(KIND_PATTERN),
            "{error} does not say what a kind name is"
        );
        assert_eq!(refused.parse::<EventKind>(), Err(error));
    }
    // The schema's own pattern, driven by a real validator, agrees with the
    // constructor on every one of those spellings.
    let validator =
        jsonschema::validator_for(&schema_of::<EventKind>()).expect("the schema compiles");
    for name in [
        "a",
        "obico_failure_alert",
        "kind_2",
        "x_",
        "",
        "Kind",
        "kind-name",
        "kind name",
        "_kind",
        "1kind",
        "kind.name",
        "kÿnd",
    ] {
        assert_eq!(
            validator.is_valid(&json!(name)),
            EventKind::new(name).is_ok(),
            "{name:?}: the schema's pattern and the constructor disagree"
        );
    }

    let mut record = serde_json::to_value(EventRecord::sample_full()).expect("a record renders");
    record["kind"] = json!("Not-A-Kind");
    let refused =
        serde_json::from_value::<EventRecord>(record).expect_err("an invalid kind parses");
    assert!(
        refused.to_string().contains("is not a kind name"),
        "{refused}"
    );
    assert!(
        matches!(EventKind::new("Not-A-Kind"), Err(EventKindError { .. })),
        "the error type is the seam's own"
    );
}
