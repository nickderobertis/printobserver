//! A print whose G-code plans colour changes, waiting at one and carrying on,
//! read through this program.
//!
//! The [`PLANNED_CHANGES`] world's stood-in machine runs the core crate's
//! committed two-colour fixture and serves its bytes, so the real server reads and
//! indexes the file itself. The journey moves only the file position the job
//! reports — onto the first `M600` line and then past its reach — and the
//! server's own printer watch records the change reached and carried on from.
//! `events`, `watch`, `status` and `context` are each the compiled program, and
//! what each says is held to the fixture's own bytes.

use printobserver_server::{Answer, operation};
use printobserver_types::serde_json::{Value, json};

use crate::machine::{Machine, Reports};
use crate::traced::Ran;
use crate::world::{PLANNED_CHANGES, Printer, TWO_COLOUR, World, committed_skill};

use super::answers::{answered, at};
use super::formats::{over_one_snapshot, the_two_renderings_agree};
use super::looking::{PATIENCE, wait_for};
use super::running;
use super::streaming::{documents, watch_with};

/// What the supervisor records when the job reaches a planned change.
const REACHED: &str = "filament_change_planned_reached";

/// What it records when the job carries on past one.
const RESUMED: &str = "filament_change_planned_resumed";

/// How far past a change's line a job may stand and still be waiting at it,
/// as the core crate's `PLANNED_CHANGE_REACH_BYTES` declares.
const REACH_BYTES: u64 = 4096;

/// The smallest interval the printer watch may poll at, so each position the
/// journey moves to is read within seconds.
const POLL_S: u64 = printobserver_server::config::PRINTER_POLL_RANGE_S.0;

/// The colour the fixture starts in.
const WHITE: &str = "#FFFFFF";

/// The colour its first change loads.
const RED: &str = "#FF0000";

/// The colour its second change loads.
const BLUE: &str = "#0000FF";

/// The byte offset of each `M600` line of the fixture, in file order.
fn change_offsets() -> Vec<u64> {
    let mut offsets = Vec::new();
    let mut start = 0;
    for line in TWO_COLOUR.split_inclusive(|byte| *byte == b'\n') {
        if line.trim_ascii() == b"M600" {
            offsets.push(u64::try_from(start).expect("an offset fits"));
        }
        start += line.len();
    }
    assert_eq!(
        offsets.len(),
        2,
        "the fixture no longer plans two colour changes"
    );
    offsets
}

/// The print's planned-change events of one kind, oldest first, read with
/// `printobserver events`.
fn events_of(world: &World, print_id: &str, kind: &str) -> Vec<Value> {
    running::read(world, &["events", "--print-id", print_id, "--kinds", kind])["events"]
        .as_array()
        .unwrap_or_else(|| panic!("an events answer carries its events"))
        .clone()
}

/// Wait for the print's first event of one kind, and answer it.
fn the_first(world: &World, print_id: &str, kind: &str) -> Value {
    wait_for(&format!("a `{kind}` event"), PATIENCE, || {
        !events_of(world, print_id, kind).is_empty()
    });
    let found = events_of(world, print_id, kind);
    assert_eq!(
        found.len(),
        1,
        "one change recorded {kind} twice: {found:#?}"
    );
    found[0].clone()
}

/// Move the machine's job to a byte of its file.
fn stand_at(machine: &Machine, position: u64) {
    machine
        .stands_at
        .store(position, std::sync::atomic::Ordering::SeqCst);
}

/// Hold a recorded change to the fixture's first: red, at layer 3, Z 0.6.
fn is_the_first_change(change: &Value, offset: u64) {
    assert_eq!(change["index"], json!(0), "{change}");
    assert_eq!(change["kind"], json!("color_change"), "{change}");
    assert_eq!(change["offset_bytes"], json!(offset), "{change}");
    assert_eq!(change["layer"], json!(3), "{change}");
    assert_eq!(change["z_mm"], json!(0.6), "{change}");
    assert_eq!(change["color"], json!(RED), "{change}");
}

/// Read the print's status in both renderings, hold them to each other — the
/// labelled lines carrying the planned changes field for field — and answer
/// its planned changes.
fn status(world: &World, print_id: &str) -> Value {
    let asked = ["status", "--print-id", print_id];
    let machine = running::command(world, &[&asked[..], &["--json"]].concat());
    let plain = running::command(world, &asked);
    let read = operation("status").expect("a status operation");
    the_two_renderings_agree(&machine, &plain, read, Answer::Success);
    let lines = answered(&plain, false);
    for field in [
        "waiting",
        "expected_color",
        "changes.0.color",
        "segments.0.color",
    ] {
        at(&lines, &format!("planned_changes.{field}"));
    }
    document_of(&machine)["planned_changes"].clone()
}

/// What one machine-readable run printed, as a document.
fn document_of(ran: &Ran) -> Value {
    printobserver_types::serde_json::from_str(&ran.out)
        .unwrap_or_else(|error| panic!("{:?} printed no document ({error})", ran.arguments))
}

/// Read the print's context in both renderings, hold them to each other, and
/// answer the document's slicer plan.
fn slicer(world: &World, print_id: &str) -> Value {
    let read = operation("context").expect("a context operation");
    let mut slicer = Value::Null;
    over_one_snapshot(
        |machine_readable| {
            let mut asked = vec!["context", "--print-id", print_id];
            if machine_readable {
                asked.push("--json");
            }
            let ran = running::command(world, &asked);
            if machine_readable {
                slicer = document_of(&ran)["context"]["slicer"].clone();
            }
            ran
        },
        read,
        Answer::Success,
    );
    slicer
}

/// Before the job reaches a change: the status read indexes the file and
/// answers both changes, the three stretches and white expected.
fn before_the_change(world: &World, print_id: &str, offsets: &[u64]) {
    let before = status(world, print_id);
    let changes = before["changes"]
        .as_array()
        .unwrap_or_else(|| panic!("the status answers no planned changes: {before}"));
    assert_eq!(changes.len(), 2, "{before}");
    is_the_first_change(&changes[0], offsets[0]);
    assert_eq!(changes[1]["offset_bytes"], json!(offsets[1]), "{before}");
    assert_eq!(changes[1]["layer"], json!(5), "{before}");
    assert_eq!(changes[1]["z_mm"], json!(1.0), "{before}");
    assert_eq!(changes[1]["color"], json!(BLUE), "{before}");
    let colours: Vec<&Value> = before["segments"]
        .as_array()
        .unwrap_or_else(|| panic!("the status answers no segments: {before}"))
        .iter()
        .map(|segment| &segment["color"])
        .collect();
    assert_eq!(
        colours,
        [&json!(WHITE), &json!(RED), &json!(BLUE)],
        "{before}"
    );
    assert_eq!(before["waiting"], Value::Null, "{before}");
    assert_eq!(before["expected_color"], json!(WHITE), "{before}");
    let plan = slicer(world, print_id);
    assert_eq!(plan["expected_color"], json!(WHITE), "{plan}");
    is_the_first_change(&plan["next_change"], offsets[0]);
    assert_eq!(plan.get("change_waiting"), None, "{plan}");
}

/// The job stands just past the first `M600` line, as a printer that has read
/// it does: the change is recorded reached, and status and context name it
/// waited at. Answers the event.
fn waiting_at_the_change(world: &World, machine: &Machine, offsets: &[u64]) -> Value {
    let print_id = world.print_id();
    let waiting_at = offsets[0] + u64::try_from(b"M600\n".len()).expect("a length fits");
    stand_at(machine, waiting_at);
    let reached = the_first(world, &print_id, REACHED);
    let payload = &reached["payload"];
    is_the_first_change(&payload["change"], offsets[0]);
    assert_eq!(payload["previous_color"], json!(WHITE), "{reached}");
    assert_eq!(payload["trigger"], json!("position"), "{reached}");
    assert_eq!(payload["position"]["bytes"], json!(waiting_at), "{reached}");
    assert_eq!(
        payload["position"]["size_bytes"],
        json!(TWO_COLOUR.len()),
        "{reached}"
    );
    assert_eq!(payload["spool_id"], Value::Null, "{reached}");
    assert_eq!(payload["next_spool_id"], Value::Null, "{reached}");
    assert!(
        payload["next_segment_filament_mm"]
            .as_f64()
            .is_some_and(|millimetres| millimetres > 0.0),
        "the red stretch feeds no filament: {reached}"
    );

    let waiting = status(world, &print_id);
    assert_eq!(waiting["waiting"], json!(0), "{waiting}");
    assert_eq!(waiting["expected_color"], json!(RED), "{waiting}");
    let plan = slicer(world, &print_id);
    is_the_first_change(&plan["change_waiting"], offsets[0]);
    assert_eq!(plan["expected_color"], json!(RED), "{plan}");
    assert_eq!(plan["next_change"]["index"], json!(1), "{plan}");
    assert!(plan["minutes_to_next_change"].is_i64(), "{plan}");
    reached
}

/// The job stands past the change's reach and short of the next: the change
/// is recorded carried on from, and nothing is waited at. Answers the event.
fn carried_on_from_the_change(world: &World, machine: &Machine, offsets: &[u64]) -> Value {
    let print_id = world.print_id();
    let carried_on_at = offsets[0] + REACH_BYTES + 512;
    assert!(
        carried_on_at < offsets[1],
        "the fixture's changes are too close"
    );
    stand_at(machine, carried_on_at);
    let resumed = the_first(world, &print_id, RESUMED);
    let payload = &resumed["payload"];
    is_the_first_change(&payload["change"], offsets[0]);
    assert_eq!(payload["previous_color"], json!(WHITE), "{resumed}");
    assert!(
        payload["waited_s"]
            .as_i64()
            .is_some_and(|waited| waited >= 0),
        "{resumed}"
    );
    assert_eq!(
        payload["position"]["bytes"],
        json!(carried_on_at),
        "{resumed}"
    );

    let carried_on = status(world, &print_id);
    assert_eq!(carried_on["waiting"], Value::Null, "{carried_on}");
    assert_eq!(carried_on["expected_color"], json!(RED), "{carried_on}");
    let plan = slicer(world, &print_id);
    assert_eq!(plan.get("change_waiting"), None, "{plan}");
    assert_eq!(plan["next_change"]["index"], json!(1), "{plan}");
    resumed
}

/// A print stands at its first planned colour change and then carries on, and
/// `events`, `watch`, `status` and `context` each say so.
#[test]
fn a_planned_colour_change_waited_at_and_carried_on_from_is_read_through_this_program() {
    let offsets = change_offsets();
    let world = World::configured(PLANNED_CHANGES, &committed_skill(), None, |document| {
        document["supervisor"]["printer_poll_s"] = json!(POLL_S);
    });
    let Printer::StoodIn(machine) = &world.printer else {
        panic!("the {PLANNED_CHANGES} world is not a stood-in machine");
    };
    world.wants(Reports::Printing);
    let print_id = world.print_id();
    let stream = watch_with(
        &[
            "--json",
            "--print-id",
            &print_id,
            "--kind",
            REACHED,
            "--kind",
            RESUMED,
        ],
        &world.client_config(),
    );

    before_the_change(&world, &print_id, &offsets);
    let reached = waiting_at_the_change(&world, machine, &offsets);
    let resumed = carried_on_from_the_change(&world, machine, &offsets);

    // The print ends, and the watch with it, having printed both events
    // exactly as `events` answers them.
    running::read(
        &world,
        &[
            "cancel",
            "--print-id",
            &print_id,
            "--actor",
            "operator",
            "--reason",
            "the colour-change journey is over",
        ],
    );
    let (lines, status) = stream.finished();
    let lines = documents(&lines);
    assert_eq!(status.code(), Some(0), "{lines:#?}");
    let printed: Vec<&Value> = lines
        .iter()
        .filter(|line| line.get("id").is_some())
        .collect();
    assert_eq!(
        printed,
        [&reached, &resumed],
        "the watch did not print the change reached and carried on from, as `events` answers them"
    );
    assert_eq!(
        lines.last().map(|line| &line["watch_ended"]["reason"]),
        Some(&json!("print_ended")),
        "{lines:#?}"
    );
}

// Waiting on the printer watch is bounded by several of its polls, so one
// slow read does not fail the journey.
const _: () = assert!(PATIENCE.as_secs() > 4 * POLL_S);
