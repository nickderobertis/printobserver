//! The event kind and its payload are one closed pair.
//!
//! Every kind has exactly one payload variant and every payload variant has
//! exactly one kind, so a consumer that matches the kind knows the payload's
//! shape. A mismatched pair is unrepresentable rather than merely
//! undocumented: the record carries the payload, the `kind` on the wire is that
//! payload's own tag, and there is no second field for a caller to disagree
//! with it in.
//!
//! The walk below is over **every ordered pair of distinct kinds**, because a
//! pair that happens to parse is exactly the case a spot check would miss.

use printobserver_types::contract::{TypeContract, declared};
use printobserver_types::{EventKind, EventPayload, EventRecord};
use serde_json::Value;

/// The contract for one declared type, by name.
fn contract(type_name: &str) -> TypeContract {
    declared()
        .into_iter()
        .find(|entry| entry.name == type_name)
        .unwrap_or_else(|| panic!("{type_name} is declared"))
}

/// One canonical value per kind, keyed by the kind it carries.
fn by_kind(entry: &TypeContract) -> Vec<(String, Value)> {
    let mut found: Vec<(String, Value)> = Vec::new();
    for value in entry.samples() {
        let kind = value
            .get("kind")
            .and_then(Value::as_str)
            .expect("every value carries its kind")
            .to_owned();
        if !found.iter().any(|(seen, _)| *seen == kind) {
            found.push((kind, value));
        }
    }
    found
}

/// The wire spelling of every kind the vocabulary declares.
fn stated_kinds() -> Vec<String> {
    EventKind::ALL
        .iter()
        .map(|kind| {
            serde_json::to_value(kind)
                .expect("a kind serializes")
                .as_str()
                .expect("a kind is a string")
                .to_owned()
        })
        .collect()
}

/// The corpus carries one value per kind the vocabulary declares.
///
/// A vocabulary that gains a kind or a payload variant this walk does not cover
/// is refused here, so the coverage cannot fall behind the vocabulary.
#[test]
fn the_corpus_carries_one_value_per_declared_kind() {
    assert_eq!(EventKind::ALL.len(), 12);
    for type_name in ["EventRecord", "EventPayload"] {
        let mut carried: Vec<String> = by_kind(&contract(type_name))
            .into_iter()
            .map(|(kind, _)| kind)
            .collect();
        carried.sort();
        let mut stated = stated_kinds();
        stated.sort();
        assert_eq!(
            carried, stated,
            "{type_name}'s corpus does not cover the vocabulary"
        );
    }
}

/// Every well-formed pair round-trips.
#[test]
fn every_well_formed_pair_round_trips() {
    for type_name in ["EventRecord", "EventPayload"] {
        let entry = contract(type_name);
        for (kind, value) in by_kind(&entry) {
            let round = entry
                .round_trip(value.clone())
                .unwrap_or_else(|error| panic!("{type_name} {kind}: {error}"));
            assert_eq!(
                round, value,
                "{type_name} {kind} does not survive a round trip"
            );
        }
    }
}

/// Every mismatched pair of distinct kinds fails to parse.
#[test]
fn every_mismatched_pair_fails_to_parse() {
    let mut driven = 0_usize;
    for type_name in ["EventRecord", "EventPayload"] {
        let entry = contract(type_name);
        let corpus = by_kind(&entry);
        for (kind, value) in &corpus {
            for (other_kind, other) in &corpus {
                if kind == other_kind {
                    continue;
                }
                let mut mismatched = value.clone();
                mismatched["payload"] = other["payload"].clone();
                assert!(
                    entry.round_trip(mismatched).is_err(),
                    "{type_name} parsed kind {kind} carrying the {other_kind} payload"
                );
                driven += 1;
            }
        }
    }
    assert_eq!(
        driven,
        2 * 12 * 11,
        "the walk did not cover every ordered pair"
    );
}

/// A record reads its kind off the payload it carries, and there is one place
/// that can disagree: none.
#[test]
fn a_record_reads_its_kind_off_its_payload() {
    let entry = contract("EventRecord");
    for (kind, value) in by_kind(&entry) {
        let record: EventRecord = serde_json::from_value(value).expect("a record parses");
        let read = serde_json::to_value(record.kind()).expect("a kind serializes");
        assert_eq!(read.as_str(), Some(kind.as_str()));
        assert_eq!(record.kind(), record.payload.kind());
    }
}

/// Every payload variant belongs to exactly one kind.
#[test]
fn every_payload_variant_belongs_to_exactly_one_kind() {
    let entry = contract("EventPayload");
    let mut kinds = Vec::new();
    for (_, value) in by_kind(&entry) {
        let payload: EventPayload = serde_json::from_value(value).expect("a payload parses");
        kinds.push(payload.kind());
    }
    kinds.sort();
    kinds.dedup();
    assert_eq!(kinds.len(), EventKind::ALL.len());
}
