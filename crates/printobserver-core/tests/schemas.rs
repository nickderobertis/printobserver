//! The event kinds this crate declares emit their schemas, marked with their kinds.
//!
//! The `printobserver-types:schemas` graph target runs this beside the other
//! declaring crates' `schemas` tests: with `PRINTOBSERVER_SCHEMAS=write` it
//! writes every kind's schema under `schemas/printobserver-core/`, and without
//! it refuses a tree whose checked-in schema no longer matches what the types
//! generate. Each schema carries the `x-event-kind` marker naming the kind, and
//! the marker is written from the payload type's own `KIND` — nothing here
//! spells a kind name a second time.

#[path = "support/schema_files.rs"]
mod schema_files;

use printobserver_core::{
    ActionExecutedPayload, ActionRejectedPayload, ActionRequestedPayload, AgentAssessmentPayload,
    InterventionExpiredPayload, OperatorAcknowledgementPayload, PortFailurePayload,
    PortFailureSite, agent_source, operator_source, system_source,
};
use printobserver_types::contract::{Sample, schema_of};
use printobserver_types::serde_json::Value;
use printobserver_types::{EVENT_KIND_MARKER, EventBody, EventPayload, event_schema_of};
use schema_files::reconcile;

/// One kind this crate declares: its schema, its marker, and its samples.
struct DeclaredKind {
    /// The type's own name, which is also its schema's file name.
    name: &'static str,
    /// The kind it is written under.
    kind: &'static str,
    /// The marked schema it emits.
    schema: Value,
    /// Every canonical value, rendered as a body and read back as the type.
    round_trips: fn() -> Vec<bool>,
}

/// One kind, from its payload type.
fn kind_of<P: EventPayload + Sample + PartialEq>(name: &'static str) -> DeclaredKind {
    fn round_trips<P: EventPayload + Sample + PartialEq>() -> Vec<bool> {
        let mut samples = vec![P::sample_full(), P::sample_minimal()];
        samples.extend(P::sample_alternates());
        samples
            .iter()
            .map(|sample| {
                let body = EventBody::of(sample).expect("a payload renders");
                body.kind.as_str() == P::KIND
                    && body.read::<P>().expect("under its own kind").ok().as_ref() == Some(sample)
            })
            .collect()
    }
    DeclaredKind {
        name,
        kind: P::KIND,
        schema: event_schema_of::<P>(),
        round_trips: round_trips::<P>,
    }
}

/// Every kind this crate declares.
fn declared_kinds() -> Vec<DeclaredKind> {
    vec![
        kind_of::<ActionRequestedPayload>("ActionRequestedPayload"),
        kind_of::<ActionExecutedPayload>("ActionExecutedPayload"),
        kind_of::<ActionRejectedPayload>("ActionRejectedPayload"),
        kind_of::<InterventionExpiredPayload>("InterventionExpiredPayload"),
        kind_of::<AgentAssessmentPayload>("AgentAssessmentPayload"),
        kind_of::<OperatorAcknowledgementPayload>("OperatorAcknowledgementPayload"),
        kind_of::<PortFailurePayload>("PortFailurePayload"),
    ]
}

/// The schemas this crate declares, by the file name each is written under.
fn generated() -> Vec<(String, Value)> {
    let mut entries: Vec<(String, Value)> = declared_kinds()
        .into_iter()
        .map(|declared| (format!("{}.json", declared.name), declared.schema))
        .collect();
    entries.push((
        "PortFailureSite.json".to_owned(),
        schema_of::<PortFailureSite>(),
    ));
    entries
}

/// The checked-in schemas of this crate are what its types generate.
#[test]
fn the_checked_in_schemas_are_what_the_types_generate() {
    let findings = reconcile("printobserver-core", &generated());
    assert!(
        findings.is_empty(),
        "the checked-in schemas have drifted:\n{}",
        findings.join("\n")
    );
}

/// Each kind is written under the name the vocabulary has always used, and its
/// schema's marker is that name.
#[test]
fn each_kind_is_written_under_its_own_name() {
    let names: Vec<(&str, &str)> = declared_kinds()
        .iter()
        .map(|declared| (declared.name, declared.kind))
        .collect();
    assert_eq!(
        names,
        [
            ("ActionRequestedPayload", "action_requested"),
            ("ActionExecutedPayload", "action_executed"),
            ("ActionRejectedPayload", "action_rejected"),
            ("InterventionExpiredPayload", "intervention_expired"),
            ("AgentAssessmentPayload", "agent_assessment"),
            ("OperatorAcknowledgementPayload", "operator_acknowledgement"),
            ("PortFailurePayload", "port_failure"),
        ]
    );
    for declared in declared_kinds() {
        assert_eq!(
            declared.schema[EVENT_KIND_MARKER].as_str(),
            Some(declared.kind),
            "{}'s schema is marked with another kind",
            declared.name
        );
        assert_eq!(
            declared.schema["title"].as_str(),
            Some(declared.name),
            "{}'s schema is titled otherwise",
            declared.name
        );
    }
}

/// Every canonical value of every kind reads back under its own kind.
#[test]
fn every_canonical_value_reads_back_under_its_own_kind() {
    for declared in declared_kinds() {
        let outcomes = (declared.round_trips)();
        assert!(
            !outcomes.is_empty() && outcomes.iter().all(|held| *held),
            "{} does not read back under its own kind: {outcomes:?}",
            declared.name
        );
    }
}

/// Nothing reads back under another of this crate's kinds.
#[test]
fn nothing_reads_back_under_another_kind() {
    let body = EventBody::of(&PortFailurePayload::sample_full()).expect("a payload renders");
    assert!(body.read::<ActionExecutedPayload>().is_none());
    assert!(body.read::<AgentAssessmentPayload>().is_none());
    assert!(body.read::<PortFailurePayload>().is_some());
}

/// The three source names are the ones the history has always carried.
#[test]
fn the_source_names_are_the_ones_the_history_carries() {
    assert_eq!(system_source().as_str(), "system");
    assert_eq!(operator_source().as_str(), "operator");
    assert_eq!(agent_source().as_str(), "agent");
}
