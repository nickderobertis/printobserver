//! The types this crate declares emit their schemas, and the event kinds among
//! them are marked with their kinds.
//!
//! The `printobserver-types:schemas` graph target runs this beside the other
//! declaring crates' `schemas` tests: with `PRINTOBSERVER_SCHEMAS=write` it
//! writes every schema under `schemas/printobserver-core/`, and without it
//! refuses a tree whose checked-in schema no longer matches what the types
//! generate. Each kind's schema carries the `x-event-kind` marker naming the
//! kind, and the marker is written from the payload type's own `KIND` —
//! nothing here spells a kind name a second time.
//!
//! The print's context is this crate's declaration too, so the fields it
//! carries, the round trip of its canonical values and the absent-optional
//! rule are walked here over it; so are the two shapes the store traits carry
//! across a process boundary, the event draft and the history query; and so
//! are this domain's own records and identifiers, whose fields and wire forms
//! the `records` test walks over the same list.

#[path = "support/records.rs"]
mod records;
#[path = "support/schema_files.rs"]
mod schema_files;

use printobserver_core::store::{EventDraft, HistoryQuery};
use printobserver_core::{
    ActionExecutedPayload, ActionRejectedPayload, ActionRequestedPayload, AgentAssessmentPayload,
    CameraLookPayload, ChargePendedPayload, FilamentChangeSignalledPayload,
    FilamentConsumedPayload, FileTallyRebaselinedPayload, InterventionExpiredPayload, Look,
    OperatorAcknowledgementPayload, PeriodicObservationPayload, PortFailurePayload,
    PortFailureSite, PreflightCheckedPayload, PrintContext, PrintEndedPayload, PrintStartedPayload,
    PrintUntrackedPayload, RebaselineDisposition, SpoolAdjustedPayload,
    SpoolCommandRejectedPayload, SpoolLoadedPayload, SpoolRegisteredPayload, SpoolRetiredPayload,
    SpoolUnloadedPayload, SpoolUnverifiedPayload, TIMELINE_KINDS, UnaccountedPrintPayload,
    agent_source, operator_source, system_source,
};
use printobserver_types::contract::{Sample, TypeContract, schema_of};
use printobserver_types::serde_json::Value;
use printobserver_types::{
    EVENT_KIND_MARKER, EventBody, EventPayload, WireField, event_schema_of, wire_fields,
};
use schema_files::reconcile;

/// The print's context, with its canonical values.
fn print_context() -> TypeContract {
    TypeContract::of::<PrintContext>("PrintContext")
}

/// A fresh look at a print, which the look operation answers, with its
/// canonical values.
fn look() -> TypeContract {
    TypeContract::of::<Look>("Look")
}

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
    /// Every canonical value, in its wire form.
    values: fn() -> Vec<Value>,
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
    fn values<P: EventPayload + Sample>() -> Vec<Value> {
        let mut samples = vec![P::sample_full(), P::sample_minimal()];
        samples.extend(P::sample_alternates());
        samples
            .iter()
            .map(|sample| EventBody::of(sample).expect("a payload renders").payload)
            .collect()
    }
    DeclaredKind {
        name,
        kind: P::KIND,
        schema: event_schema_of::<P>(),
        round_trips: round_trips::<P>,
        values: values::<P>,
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
        kind_of::<CameraLookPayload>("CameraLookPayload"),
        kind_of::<PeriodicObservationPayload>("PeriodicObservationPayload"),
        kind_of::<PrintStartedPayload>("PrintStartedPayload"),
        kind_of::<PrintEndedPayload>("PrintEndedPayload"),
        kind_of::<FilamentConsumedPayload>("FilamentConsumedPayload"),
        kind_of::<SpoolRegisteredPayload>("SpoolRegisteredPayload"),
        kind_of::<SpoolLoadedPayload>("SpoolLoadedPayload"),
        kind_of::<SpoolUnloadedPayload>("SpoolUnloadedPayload"),
        kind_of::<SpoolAdjustedPayload>("SpoolAdjustedPayload"),
        kind_of::<SpoolRetiredPayload>("SpoolRetiredPayload"),
        kind_of::<SpoolCommandRejectedPayload>("SpoolCommandRejectedPayload"),
        kind_of::<SpoolUnverifiedPayload>("SpoolUnverifiedPayload"),
        kind_of::<FilamentChangeSignalledPayload>("FilamentChangeSignalledPayload"),
        kind_of::<UnaccountedPrintPayload>("UnaccountedPrintPayload"),
        kind_of::<PreflightCheckedPayload>("PreflightCheckedPayload"),
        kind_of::<ChargePendedPayload>("ChargePendedPayload"),
        kind_of::<PrintUntrackedPayload>("PrintUntrackedPayload"),
        kind_of::<FileTallyRebaselinedPayload>("FileTallyRebaselinedPayload"),
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
    entries.push((
        "RebaselineDisposition.json".to_owned(),
        schema_of::<RebaselineDisposition>(),
    ));
    let context = print_context();
    entries.push((format!("{}.json", context.name), context.schema()));
    let look = look();
    entries.push((format!("{}.json", look.name), look.schema()));
    entries.push(("EventDraft.json".to_owned(), schema_of::<EventDraft>()));
    entries.push(("HistoryQuery.json".to_owned(), schema_of::<HistoryQuery>()));
    entries.extend(
        records::declared()
            .into_iter()
            .map(|entry| (format!("{}.json", entry.name), entry.schema())),
    );
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
            ("CameraLookPayload", "camera_look"),
            ("PeriodicObservationPayload", "periodic_observation"),
            ("PrintStartedPayload", "print_started"),
            ("PrintEndedPayload", "print_ended"),
            ("FilamentConsumedPayload", "filament_consumed"),
            ("SpoolRegisteredPayload", "spool_registered"),
            ("SpoolLoadedPayload", "spool_loaded"),
            ("SpoolUnloadedPayload", "spool_unloaded"),
            ("SpoolAdjustedPayload", "spool_adjusted"),
            ("SpoolRetiredPayload", "spool_retired"),
            ("SpoolCommandRejectedPayload", "spool_command_rejected"),
            ("SpoolUnverifiedPayload", "spool_unverified"),
            (
                "FilamentChangeSignalledPayload",
                "filament_change_signalled"
            ),
            ("UnaccountedPrintPayload", "unaccounted_print"),
            ("PreflightCheckedPayload", "preflight_checked"),
            ("ChargePendedPayload", "charge_pended"),
            ("PrintUntrackedPayload", "print_untracked"),
            ("FileTallyRebaselinedPayload", "file_tally_rebaselined"),
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

/// The printer timeline is exactly the sixteen kinds the ledger declares, in
/// the order the contract lists them, each written under its payload's own
/// kind.
#[test]
fn the_timeline_is_exactly_the_ledgers_sixteen_kinds() {
    let ledger: Vec<&str> = declared_kinds()
        .iter()
        .skip_while(|declared| declared.name != "PrintStartedPayload")
        .map(|declared| declared.kind)
        .collect();
    assert_eq!(TIMELINE_KINDS.to_vec(), ledger);
    assert_eq!(
        TIMELINE_KINDS,
        [
            "print_started",
            "print_ended",
            "filament_consumed",
            "spool_registered",
            "spool_loaded",
            "spool_unloaded",
            "spool_adjusted",
            "spool_retired",
            "spool_command_rejected",
            "spool_unverified",
            "filament_change_signalled",
            "unaccounted_print",
            "preflight_checked",
            "charge_pended",
            "print_untracked",
            "file_tally_rebaselined",
        ]
    );
}

/// Every canonical value of every kind is one its own generated schema admits.
#[test]
fn every_canonical_value_of_every_kind_validates_against_its_schema() {
    for declared in declared_kinds() {
        let validator =
            jsonschema::validator_for(&declared.schema).expect("the generated schema compiles");
        for value in (declared.values)() {
            assert!(
                validator.is_valid(&value),
                "{}'s schema refused {value}",
                declared.name
            );
        }
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

/// The field the contract states, as a name, what it is, and whether it is
/// required.
fn field(name: &str, descriptor: &str, required: bool) -> WireField {
    WireField {
        name: name.to_owned(),
        descriptor: descriptor.to_owned(),
        required,
    }
}

/// The print's context carries exactly the fields the contract states.
#[test]
fn the_print_context_carries_exactly_the_stated_fields() {
    let context = print_context();
    assert_eq!(
        wire_fields(&context.schema()),
        vec![
            field("bounds", "EffectiveBounds", true),
            field("interventions", "array:Intervention", true),
            field("job", "JobSnapshot", false),
            field("latest_image", "ImageRef", false),
            field("manifest", "JobManifest", false),
            field("print", "PrintRecord", true),
            field("printer", "PrinterSnapshot", false),
            field("recent_events", "array:EventRecord", true),
        ]
    );
    assert_eq!(
        context.schema().get("title").and_then(Value::as_str),
        Some("PrintContext")
    );
}

/// A look carries exactly the fields the contract states, round-trips its
/// canonical values, and omits what it does not carry rather than writing
/// `null`.
#[test]
fn a_look_carries_exactly_the_stated_fields() {
    let look = look();
    assert_eq!(
        wire_fields(&look.schema()),
        vec![
            field("arrived", "array:EventRecord", false),
            field("detector_paused", "boolean", true),
            field("event", "EventRecord", true),
            field("frame", "ImageRef", false),
            field("image_path", "string", false),
            field("job", "JobSnapshot", false),
            field("printer", "PrinterSnapshot", false),
        ]
    );
    for value in look.samples() {
        let round = look
            .round_trip(value.clone())
            .unwrap_or_else(|error| panic!("Look: {error}"));
        assert_eq!(round, value, "Look does not survive a round trip");
    }
    let minimal = look.minimal();
    let object = minimal.as_object().expect("an object");
    for name in ["arrived", "frame", "image_path", "printer", "job"] {
        assert!(
            !object.contains_key(name),
            "the minimal look carries {name}"
        );
    }
}

/// The print's context round-trips its canonical values unchanged, and an
/// optional it does not carry is absent from the serialized object rather
/// than `null`: a snapshot that could not be taken is one the agent is not
/// shown.
#[test]
fn the_print_context_round_trips_and_omits_what_it_does_not_carry() {
    let context = print_context();
    for value in context.samples() {
        let round = context
            .round_trip(value.clone())
            .unwrap_or_else(|error| panic!("PrintContext: {error}"));
        assert_eq!(round, value, "PrintContext does not survive a round trip");
    }
    let minimal = context.minimal();
    let object = minimal.as_object().expect("an object");
    for name in ["printer", "job", "manifest", "latest_image"] {
        assert!(
            !object.contains_key(name),
            "the minimal context carries {name} as {:?}",
            object.get(name)
        );
    }
    let nonsense = printobserver_types::serde_json::json!({ "print": "not a record" });
    assert!(context.round_trip(nonsense).is_err());
}

/// The event draft carries exactly the fields the contract states: the
/// envelope's own less the identifier and the image the store adds.
#[test]
fn the_event_draft_carries_exactly_the_stated_fields() {
    let expected = vec![
        field("kind", "EventKind", true),
        field("payload", "any", true),
        field("print_id", "PrintId", false),
        field("raw", "RawBytes", false),
        field("received_at", "Timestamp", true),
        field("source", "EventSource", true),
    ];
    assert_eq!(wire_fields(&schema_of::<EventDraft>()), expected);
}

/// The history query carries exactly the fields the contract states.
#[test]
fn the_history_query_carries_exactly_the_stated_fields() {
    let expected = vec![
        field("kinds", "array:EventKind", true),
        field("limit", "integer", false),
        field("print_id", "PrintId", true),
        field("since", "Timestamp", false),
        field("until", "Timestamp", false),
    ];
    assert_eq!(wire_fields(&schema_of::<HistoryQuery>()), expected);
}
