//! The one journey all three clients drive, in Rust.
//!
//! Nine steps against a **real supervisor backed by a real `OctoPrint`** — the
//! instance `just octoprint-up` started, with its virtual printer. Nothing here
//! stands in for a layer: the client is the published one, the server is the
//! program this repository builds, and the machine is a real one.
//!
//! The same nine steps run in the Python and the Node clients, in the same
//! order, asserting the same normalized answers. Three clients running three
//! different journeys would prove three different products.
//!
//! # Four orderings are load-bearing
//!
//! The manifest is written before the print is started, so the print runs under
//! it. The accepted adjustment and the rejected one both happen after the print
//! has started and before history is read, so history has them to account for.
//! The print is cancelled last.
//!
//! # It puts the hold print back
//!
//! `just test-integration` runs this beside the scripted environment's own
//! suite, which asserts a print is there to be acted on. This walk cancels one
//! and starts one, so it leaves the machine where the bring-up left it.

#[path = "support/supervisor.rs"]
mod supervisor;

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use printobserver_sdk::{
    Actor, Client, ClientError, EventRecordKind, ExecutionOutcome, JobManifest, PolicyDecision,
    PrintAction, PrinterState, Range, RejectionReason,
};

/// The reason every mutating step of this walk carries.
const REASON: &str = "a printer-integration journey is asking";

/// How long a bounded adjustment stands for, in whole seconds.
const DURATION: i64 = 60;

/// The feedrate factor the accepted adjustment asks for, inside the envelope
/// the world is configured with.
const INSIDE: f64 = 1.1;

/// The feedrate factor the rejected adjustment asks for, outside it.
const OUTSIDE: f64 = 9.9;

/// How long the machine is given to reach a state a step needs.
const PATIENCE: Duration = Duration::from_secs(180);

/// The manifest this walk writes, which the print then runs under.
fn manifest(file_name: &str) -> JobManifest {
    let mut allowed = BTreeMap::new();
    allowed.insert("feedrate".to_owned(), Range { min: 0.9, max: 1.2 });
    JobManifest {
        file_name: file_name.to_owned(),
        material: "PLA".to_owned(),
        nozzle_diameter_mm: 0.4,
        slicer_profile: "a journey's own profile".to_owned(),
        allowed,
        metadata: BTreeMap::new(),
    }
}

/// Wait until the machine reports one of these states, and answer which.
///
/// # Panics
///
/// Panics when it does not, saying what it reported instead: a walk that went
/// on regardless would assert against a machine that was somewhere else.
fn until(client: &Client, print_id: &str, wanted: &[PrinterState]) -> PrinterState {
    let deadline = Instant::now() + PATIENCE;
    let mut last = PrinterState::Unknown("nothing was reported".to_owned());
    while Instant::now() < deadline {
        let status = client.status(print_id).expect("a status read is answered");
        if let Some(printer) = status.printer {
            last = printer.connection.clone();
            if wanted.contains(&printer.connection) {
                return printer.connection;
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    panic!("the machine reported {last:?} and this step needs one of {wanted:?}");
}

/// The nine steps, in the one order a real machine admits.
#[test]
fn the_same_nine_steps_are_answered_against_a_real_octoprint() {
    let root = tempfile::tempdir().expect("this journey's own root");
    let mut standing = supervisor::standing(root.path());
    let world = standing.at.clone();
    let client = Client::new(&world.server, Actor::Operator);

    reads(&client, &world);
    let wanted = the_manifest_the_print_runs_under(&client, &world);
    starts(&client, &world, &wanted);
    adjustments(&client, &world);
    accounting(&client, &world);
    cancels(&client, &world, &wanted);

    standing.stop();
}

/// Steps one and two: read status, and read context and materialize its image.
fn reads(client: &Client, world: &supervisor::Supervisor) {
    let status = client
        .status(&world.print_id)
        .expect("a status read is answered");
    assert_eq!(status.print.id, world.print_id);

    let context = client
        .context(&world.print_id)
        .expect("a context read is answered");
    assert_eq!(context.context.print.id, world.print_id);

    let image = client
        .image(&world.image_id)
        .expect("an image read is answered");
    let path = image.path.as_ref().expect("the image is on this host");
    assert!(
        std::path::Path::new(path).is_absolute(),
        "an image is an absolute path on the server's own host: {path}"
    );
    assert_eq!(
        digest(std::path::Path::new(path)),
        image.record.sha256,
        "the file at the answered path is not the image the record declares"
    );
}

/// Step three: write a job manifest and read it back, before anything starts.
fn the_manifest_the_print_runs_under(
    client: &Client,
    world: &supervisor::Supervisor,
) -> JobManifest {
    let wanted = manifest(&world.file_name);
    let written = client
        .manifest_set(&world.print_id, REASON, &wanted)
        .expect("a manifest write is answered");
    assert_eq!(written.manifest.as_ref(), Some(&wanted));
    let read_back = client
        .manifest_get(&world.print_id)
        .expect("a manifest read is answered");
    assert_eq!(read_back.manifest, written.manifest);
    wanted
}

/// Step four: start a print, under the manifest step three wrote.
fn starts(client: &Client, world: &supervisor::Supervisor, wanted: &JobManifest) {
    // The bring-up left a print running, and a machine already printing cannot
    // be started. Setting it down is this walk's own setup rather than one of
    // the nine steps, and the walk starts one again at the end.
    let _ = client.cancel(&world.print_id, "making room for the step that starts one");
    until(client, &world.print_id, &[PrinterState::Operational]);

    let started = client
        .start_print(&world.print_id, &world.file_name, wanted, REASON)
        .expect("a start is answered");
    assert!(matches!(started.record.decision, PolicyDecision::Accepted));
    until(client, &world.print_id, &[PrinterState::Printing]);
}

/// Steps five, six and seven: one accepted adjustment, one refused, and one
/// this client will not send at all.
fn adjustments(client: &Client, world: &supervisor::Supervisor) {
    let adjusted = client
        .set_feedrate_factor(&world.print_id, INSIDE, REASON, Some(DURATION))
        .expect("an adjustment inside the bounds is answered");
    assert!(matches!(adjusted.record.decision, PolicyDecision::Accepted));
    let intervention = adjusted
        .intervention
        .as_ref()
        .expect("a bounded adjustment opens an intervention");
    assert!(
        (intervention.applied_value - INSIDE).abs() < f64::EPSILON,
        "the intervention applied {} rather than {INSIDE}",
        intervention.applied_value
    );

    let refused = client
        .set_feedrate_factor(&world.print_id, OUTSIDE, REASON, None)
        .expect_err("an adjustment outside the bounds is refused");
    let ClientError::Rejected(rejection) = refused else {
        panic!("the policy's refusal did not arrive as one: {refused}");
    };
    assert!(matches!(
        rejection.reason,
        RejectionReason::OutOfBounds { .. }
    ));
    assert_eq!(rejection.requested, Some(OUTSIDE));
    let allowed = rejection
        .allowed
        .as_ref()
        .expect("the range that is allowed");
    assert!(
        allowed.max < OUTSIDE,
        "what is allowed does not admit what was asked for: {allowed:?}"
    );

    unreasoned(client, world);
}

/// Step seven on its own: a mutating call whose reason is empty reaches no
/// server at all.
fn unreasoned(client: &Client, world: &supervisor::Supervisor) {
    let before = client
        .history(&world.print_id, None)
        .expect("a history read is answered")
        .events
        .len();
    let refused = client
        .pause(&world.print_id, "   ")
        .expect_err("a call with no reason is refused");
    assert!(matches!(refused, ClientError::NoReason));

    // Pointed at an address nothing is listening on, the same call still
    // refuses for want of a reason rather than for want of a server — which is
    // what "no request reached the server" means.
    let nowhere = Client::new("127.0.0.1:1", Actor::Operator);
    assert!(matches!(
        nowhere
            .pause(&world.print_id, "")
            .expect_err("a call with no reason is refused"),
        ClientError::NoReason
    ));
    assert_eq!(
        client
            .history(&world.print_id, None)
            .expect("a history read is answered")
            .events
            .len(),
        before,
        "a call with no reason left something in the history"
    );
}

/// Step eight: read history, and find the accepted action, its decision and
/// its outcome.
fn accounting(client: &Client, world: &supervisor::Supervisor) {
    let history = client
        .history(&world.print_id, Some(200))
        .expect("a history read is answered");
    let asked = history
        .events
        .iter()
        .filter_map(|event| match &event.kind {
            EventRecordKind::ActionRequested { payload } => Some(payload),
            _ => None,
        })
        .find(|payload| {
            matches!(
                payload.action,
                PrintAction::SetFeedrateFactor { factor, .. }
                    if (factor - INSIDE).abs() < f64::EPSILON
            )
        })
        .expect("the accepted adjustment is in the history");

    // Its decision: it is in the history as requested and not as rejected.
    assert!(
        !history.events.iter().any(|event| matches!(
            &event.kind,
            EventRecordKind::ActionRejected { payload } if payload.action_id == asked.action_id
        )),
        "the accepted adjustment is in the history as a rejected one"
    );
    // And its outcome: it reached the machine.
    assert!(
        history.events.iter().any(|event| matches!(
            &event.kind,
            EventRecordKind::ActionExecuted { payload } if payload.action_id == asked.action_id
        )),
        "the history accounts for the accepted adjustment reaching no machine"
    );
    assert!(
        history.events.iter().any(|event| matches!(
            &event.kind,
            EventRecordKind::ActionRejected { payload }
                if matches!(payload.decision, PolicyDecision::Rejected(_))
        )),
        "the history accounts for no refused action"
    );
    // And the alert this print was opened by: a history that had lost the
    // event the print exists because of would be one nobody could read back.
    assert!(
        history
            .events
            .iter()
            .any(|event| event.id == world.event_id),
        "the history does not account for the event this print was opened by"
    );
}

/// Step nine: cancel the print, last — and then put the hold print back, which
/// is teardown rather than a tenth step.
fn cancels(client: &Client, world: &supervisor::Supervisor, wanted: &JobManifest) {
    let cancelled = client
        .cancel(&world.print_id, REASON)
        .expect("a cancel is answered");
    assert!(matches!(
        cancelled.record.decision,
        PolicyDecision::Accepted
    ));
    assert!(matches!(
        cancelled.record.outcome,
        Some(ExecutionOutcome::Succeeded) | None
    ));

    until(client, &world.print_id, &[PrinterState::Operational]);
    let _ = client.start_print(
        &world.print_id,
        &world.file_name,
        wanted,
        "putting the hold print back where the bring-up left it",
    );
}

/// The digest of one file's bytes, in lowercase hexadecimal.
fn digest(path: &std::path::Path) -> String {
    use std::fmt::Write as _;

    use sha2::Digest as _;

    let mut hasher = sha2::Sha256::new();
    hasher.update(std::fs::read(path).expect("the image at the answered path opens"));
    hasher
        .finalize()
        .iter()
        .fold(String::new(), |mut said, byte| {
            let _ = write!(said, "{byte:02x}");
            said
        })
}
