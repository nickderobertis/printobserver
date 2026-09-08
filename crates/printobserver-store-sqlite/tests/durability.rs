//! What the store wrote outlives the process that wrote it.
//!
//! The walk runs in a process of its own, which then ends; only afterwards does
//! this process open the same state directory and read each record back. A
//! store holding these records in process memory, or behind a cache keyed by the
//! state directory, satisfies a reopened handle and fails this — and the record
//! of record is what a person reads after the print that went wrong, by which
//! time the process that wrote it has ended.

#[path = "support/block_on.rs"]
mod block_on;
#[path = "support/child.rs"]
mod child;
#[path = "support/contracts.rs"]
mod contracts;
#[path = "support/fixture.rs"]
mod fixture;
#[path = "support/surface.rs"]
mod surface;

use std::collections::BTreeSet;

use block_on::block_on;
use contracts::record_kinds;
use fixture::{draft, instant, manifest, request, session};
use printobserver_store_api::{HistoryQuery, ImageLookup, StorePort};
use printobserver_store_sqlite::SqliteStore;
use printobserver_types::serde_json::{self, Value, json};
use printobserver_types::{
    ActionId, Adjustable, EventId, ExecutionOutcome, ImageId, InterventionId, PolicyDecision,
    PrintId, RawBytes, Timestamp,
};

/// The line the child writes what it wrote on.
const MARKER: &str = "PRINTOBSERVER-RECORDS ";

/// The instant every record the caller times carries.
const WRITTEN_AT: &str = "2026-03-01T12:00:00.123456789Z";

/// A later instant, for the intervention's expiry.
const EXPIRES_AT: &str = "2026-03-01T12:30:00.987654321Z";

/// One record kind's identifiers and instants, as the child reported them.
fn record<'a>(reported: &'a Value, kind: &str) -> &'a Value {
    reported
        .get(kind)
        .unwrap_or_else(|| panic!("the walk did not report a {kind}: {reported}"))
}

/// One reported string.
fn field(record: &Value, name: &str) -> String {
    record
        .get(name)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("the walk reported no {name} in {record}"))
        .to_owned()
}

/// One reported identifier or instant, parsed.
fn parsed<T: std::str::FromStr>(record: &Value, name: &str) -> T
where
    T::Err: core::fmt::Debug,
{
    field(record, name)
        .parse()
        .unwrap_or_else(|error| panic!("the walk reported an unreadable {name}: {error:?}"))
}

/// Every record kind the contracts declare that this walk does not cover.
fn uncovered(walked: &BTreeSet<String>) -> Vec<String> {
    record_kinds().difference(walked).cloned().collect()
}

/// Every record kind survives the process that wrote it.
#[test]
fn every_record_kind_survives_the_process_that_wrote_it() {
    let dir = tempfile::TempDir::new().expect("a temporary state directory");
    let output = child::run("the_child_writes_one_of_every_record_kind", dir.path());
    assert!(
        output.status.success(),
        "the walk did not complete: {}",
        child::reported(&output)
    );
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let line = stdout
        .lines()
        .find_map(|line| line.strip_prefix(MARKER))
        .unwrap_or_else(|| panic!("the walk reported nothing: {}", child::reported(&output)));
    let reported: Value = serde_json::from_str(line).expect("the walk reports JSON");

    let walked: BTreeSet<String> = reported
        .as_object()
        .expect("the walk reports one record per kind")
        .keys()
        .cloned()
        .collect();
    assert_eq!(
        uncovered(&walked),
        Vec::<String>::new(),
        "the contracts declare a record kind this walk does not write and read back"
    );

    // The process that wrote them has ended; this one opens the same state
    // directory for the first time.
    let store = SqliteStore::open(dir.path()).expect("the store opens");
    let port: &dyn StorePort = &store;

    for kind in walked {
        read_one_back(port, &kind, &reported);
    }
}

/// Read one record kind back, by the identifier the walk reported for it.
fn read_one_back(port: &dyn StorePort, kind: &str, reported: &Value) {
    match kind {
        "PrintRecord" => read_print_back(port, record(reported, kind)),
        "EventRecord" => read_event_back(port, record(reported, kind)),
        "ImageRecord" => read_image_back(port, record(reported, kind)),
        "ActionRecord" => read_action_back(port, record(reported, kind)),
        "Intervention" => read_intervention_back(port, record(reported, kind)),
        "JobManifest" => read_manifest_back(port, record(reported, kind)),
        "SupervisionSession" => read_session_back(port, record(reported, kind)),
        other => panic!("the walk reported {other}, which this read-back does not know"),
    }
}

/// The print survives, carrying the identifier and the instant it was written with.
fn read_print_back(port: &dyn StorePort, reported: &Value) {
    let id: PrintId = parsed(reported, "id");
    let print = block_on(port.print(id))
        .expect("the print reads")
        .expect("the print survived the process that wrote it");
    assert_eq!(print.id, id, "the print answers another print's identifier");
    assert_eq!(print.opened_at, parsed::<Timestamp>(reported, "opened_at"));
    assert_eq!(print.file_name.as_deref(), Some("bracket.gcode"));
}

/// The event survives, at the instant it was received.
fn read_event_back(port: &dyn StorePort, reported: &Value) {
    let id: EventId = parsed(reported, "id");
    let print_id: PrintId = parsed(reported, "print_id");
    let history = block_on(port.history(HistoryQuery {
        print_id,
        kinds: Vec::new(),
        since: None,
        until: None,
        limit: None,
    }))
    .expect("the history reads");
    let event = history
        .iter()
        .find(|event| event.id == id)
        .expect("the event survived the process that wrote it");
    assert_eq!(event.received_at, instant(WRITTEN_AT));
}

/// The image survives, and so do the bytes its row points at.
fn read_image_back(port: &dyn StorePort, reported: &Value) {
    let id: ImageId = parsed(reported, "id");
    let image = match block_on(port.image(id)).expect("the image reads") {
        ImageLookup::Found { record, .. } => record,
        ImageLookup::FileMissing { record } => {
            panic!("the bytes of {} did not survive: {record:?}", record.id)
        }
    };
    assert_eq!(image.id, id, "the image answers another image's identifier");
    assert_eq!(
        image.fetched_at,
        parsed::<Timestamp>(reported, "fetched_at")
    );
    assert_eq!(image.sha256, field(reported, "sha256"));
}

/// The action survives, with the decision taken on it.
///
/// The port answers an action only by recording what the printer made of it,
/// and what it answers is read out of the row the walk wrote.
fn read_action_back(port: &dyn StorePort, reported: &Value) {
    let id: ActionId = parsed(reported, "id");
    let action = block_on(port.record_execution(id, ExecutionOutcome::Succeeded))
        .expect("the action survived the process that wrote it");
    assert_eq!(
        action.id, id,
        "the action answers another action's identifier"
    );
    assert_eq!(action.request.requested_at, instant(WRITTEN_AT));
    assert_eq!(action.decision, PolicyDecision::Accepted);
}

/// The intervention survives, still active, at the instants it was written with.
fn read_intervention_back(port: &dyn StorePort, reported: &Value) {
    let id: InterventionId = parsed(reported, "id");
    let print_id: PrintId = parsed(reported, "print_id");
    let active = block_on(port.active_interventions(print_id)).expect("the interventions read");
    let intervention = active
        .iter()
        .find(|intervention| intervention.id == id)
        .expect("the intervention survived the process that wrote it");
    assert_eq!(intervention.applied_at, instant(WRITTEN_AT));
    assert_eq!(intervention.expires_at, instant(EXPIRES_AT));
}

/// The manifest survives, keyed to its print.
fn read_manifest_back(port: &dyn StorePort, reported: &Value) {
    let print_id: PrintId = parsed(reported, "print_id");
    assert_eq!(
        block_on(port.manifest(print_id)).expect("the manifest reads"),
        Some(manifest()),
        "the manifest did not survive the process that wrote it"
    );
}

/// The supervision session survives, at the instant it was opened.
fn read_session_back(port: &dyn StorePort, reported: &Value) {
    let print_id: PrintId = parsed(reported, "print_id");
    let watch = block_on(port.session(print_id))
        .expect("the session reads")
        .expect("the session survived the process that wrote it");
    assert_eq!(watch.print_id, print_id);
    assert_eq!(watch.created_at, instant(WRITTEN_AT));
}

/// The coverage check refuses a walk that misses a record kind.
#[test]
fn the_coverage_check_refuses_a_walk_that_misses_a_record_kind() {
    let mut walked = record_kinds();
    assert!(
        walked.remove("ImageRecord"),
        "the derivation does not name the record kind this fixture drops"
    );
    assert_eq!(
        uncovered(&walked),
        vec!["ImageRecord".to_owned()],
        "the check did not name the record kind the walk missed"
    );
}

/// Write one record of every kind the contracts declare, then end.
///
/// Run in a process of its own by the journey above; ignored so that nothing
/// else runs it.
#[test]
#[ignore = "run in a process of its own by every_record_kind_survives_the_process_that_wrote_it"]
fn the_child_writes_one_of_every_record_kind() {
    let dir = child::state_dir();
    let store = SqliteStore::open(&dir).expect("the store opens");
    let port: &dyn StorePort = &store;
    let at: Timestamp = instant(WRITTEN_AT);

    let print = block_on(port.open_print(Some(7), Some("bracket.gcode".to_owned())))
        .expect("a print opens");
    let event = block_on(port.append_event(draft(Some(print.id), "obico_failure_alert", at)))
        .expect("an event is appended");
    let image = block_on(port.put_image(
        print.id,
        event.id,
        Some("https://obico.example/snapshot.jpg".to_owned()),
        "image/jpeg".to_owned(),
        RawBytes::new(b"the surviving snapshot".to_vec()),
    ))
    .expect("an image is stored");
    let action = block_on(port.record_action(request(at), PolicyDecision::Accepted))
        .expect("an action is recorded");
    let intervention = block_on(port.open_intervention(
        action.id,
        Adjustable::Feedrate,
        Some(1.0),
        0.8,
        at,
        instant(EXPIRES_AT),
    ))
    .expect("an intervention opens");
    block_on(port.put_manifest(print.id, manifest())).expect("a manifest is stored");
    block_on(port.put_session(session(print.id, at))).expect("a session is stored");

    let reported = json!({
        "PrintRecord": {"id": print.id.to_string(), "opened_at": print.opened_at.to_string()},
        "EventRecord": {"id": event.id.to_string(), "print_id": print.id.to_string()},
        "ImageRecord": {
            "id": image.id.to_string(),
            "fetched_at": image.fetched_at.to_string(),
            "sha256": image.sha256,
        },
        "ActionRecord": {"id": action.id.to_string()},
        "Intervention": {
            "id": intervention.id.to_string(),
            "print_id": print.id.to_string(),
        },
        "JobManifest": {"print_id": print.id.to_string()},
        "SupervisionSession": {"print_id": print.id.to_string()},
    });
    println!("{MARKER}{reported}");
}
