//! An operator closes a print record printobserver left open.
//!
//! Every close here is the real `printobserver close-print`, against the real
//! supervisor, over a store holding the records it is about — each opened
//! with a supervision session of its own, which is what an abandoned record
//! looks like — and every outcome is read back through `prints`, `status` and
//! `history`. The machine is the stood-in one, told what to report: a job of
//! another file, a job of the record's own file printing, paused or
//! cancelling, nothing running, or no answer at all.
//!
//! What a close that went through owes is the request and its execution in
//! the record's history, then one `print_ended` with the operator's outcome,
//! the state the machine reported on that read and the operator's reason; an
//! end on the record; its session closed; and no charge. What every refusal
//! owes is the record exactly as it was.

use printobserver::failure::Exit;
use printobserver_types::serde_json::Value;

use crate::machine::{RUNNING_FILE, Reports};
use crate::world::{STALE_FILE, World};

use super::running;

/// The reason every close here gives.
const REASON: &str = "the printer was switched off mid-print and the print is over";

/// Why a session the journey opens already closed was closed.
const EARLIER_CLOSE: &str = "the harness closed this session before the record was";

/// What the record's end reason and its `print_ended` say for that reason.
fn closed_for() -> String {
    format!("closed by the operator: {REASON}")
}

/// Close one record, asking for one outcome, with one reason.
fn close(world: &World, print: &str, outcome: &str, reason: &str) -> crate::traced::Ran {
    world.proxy.forget();
    running::command(
        world,
        &[
            "close-print",
            "--print-id",
            print,
            "--outcome",
            outcome,
            "--reason",
            reason,
            "--actor",
            "operator",
            "--json",
        ],
    )
}

/// One record as `status` answers it.
fn status(world: &World, print: &str) -> Value {
    running::read(world, &["status", "--print-id", print])
}

/// One record as the `prints` listing answers it.
fn listed(world: &World, print: &str) -> Value {
    let listing = running::read(world, &["prints"]);
    listing["prints"]
        .as_array()
        .and_then(|prints| prints.iter().find(|found| found["id"] == print))
        .cloned()
        .unwrap_or_else(|| panic!("the listing does not carry {print}: {listing}"))
}

/// One record's history, newest first.
fn history(world: &World, print: &str) -> Vec<Value> {
    running::read(world, &["history", "--print-id", print, "--limit", "100"])["events"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// The events of one kind in one record's history.
fn of_kind(world: &World, print: &str, kind: &str) -> Vec<Value> {
    history(world, print)
        .into_iter()
        .filter(|event| event["kind"] == kind)
        .collect()
}

/// The events of one kind about one action, in one record's history.
fn about(world: &World, print: &str, kind: &str, action_id: &str) -> Vec<Value> {
    of_kind(world, print, kind)
        .into_iter()
        .filter(|event| event["payload"]["action_id"] == action_id)
        .collect()
}

/// The record is open, by both reads, and so is its session.
#[track_caller]
fn still_open(world: &World, print: &str) {
    let read = status(world, print);
    assert!(
        read["print"].get("ended_at").is_none(),
        "{print} was ended: {read}"
    );
    assert!(
        read["print"].get("end_reason").is_none(),
        "{print} was ended: {read}"
    );
    assert!(
        read["session"].is_object() && read["session"].get("closed_at").is_none(),
        "{print}'s session is not open: {read}"
    );
    assert!(
        listed(world, print).get("ended_at").is_none(),
        "{print} is listed ended"
    );
    assert!(
        of_kind(world, print, "print_ended").is_empty(),
        "{print} has a print_ended"
    );
    assert!(
        of_kind(world, print, "supervision_session_closed").is_empty(),
        "{print}'s session was closed"
    );
}

/// A close the policy rejected for one reason, recorded and nothing else.
///
/// Answers the rejection's own payload.
#[track_caller]
fn rejected(world: &World, print: &str, ran: &crate::traced::Ran, reason: &str) -> Value {
    assert_eq!(
        ran.code,
        Some(i32::from(Exit::Rejected.status())),
        "the close was not rejected: {}",
        ran.said()
    );
    let answer: Value = printobserver_types::serde_json::from_str(&ran.out)
        .unwrap_or_else(|error| panic!("the rejection is not a document ({error}): {}", ran.out));
    let action_id = answer["record"]["id"].as_str().expect("the record's id");
    assert_eq!(
        answer["record"]["print_id"], print,
        "the rejection was recorded against another print: {answer}"
    );
    // A rejection carrying no fields is its own name, and any other carries
    // its fields under that name.
    let decided = &answer["record"]["decision"]["rejected"];
    let refusal = if decided == reason {
        Value::Null
    } else {
        decided[reason].clone()
    };
    assert!(
        decided == reason || refusal.is_object(),
        "the close was rejected for another reason: {answer}"
    );
    assert_eq!(
        about(world, print, "action_rejected", action_id).len(),
        1,
        "the rejection is not in the record's history"
    );
    assert!(
        about(world, print, "action_executed", action_id).is_empty(),
        "a rejected close was executed"
    );
    refusal
}

/// A close that went through: its request and execution in the history, one
/// `print_ended` saying how, in what state and why, an end on the record, its
/// session closed, and no charge.
#[track_caller]
fn closed(world: &World, print: &str, ran: &crate::traced::Ran, outcome: &str, state: &str) {
    assert_eq!(
        ran.code,
        Some(0),
        "the close did not go through: {}",
        ran.said()
    );
    let answer: Value = printobserver_types::serde_json::from_str(&ran.out)
        .unwrap_or_else(|error| panic!("the close is not a document ({error}): {}", ran.out));
    let action_id = answer["record"]["id"].as_str().expect("the record's id");
    assert_eq!(
        answer["record"]["print_id"], print,
        "the close was recorded against another print: {answer}"
    );
    let requested = about(world, print, "action_requested", action_id);
    assert_eq!(
        requested.len(),
        1,
        "the close's request is not in the history"
    );
    assert_eq!(requested[0]["payload"]["action"]["action"], "close_print");
    assert_eq!(requested[0]["payload"]["action"]["outcome"], outcome);
    assert_eq!(
        about(world, print, "action_executed", action_id).len(),
        1,
        "the close's execution is not in the history"
    );

    let ended = of_kind(world, print, "print_ended");
    assert_eq!(
        ended.len(),
        1,
        "the close did not end the record once: {ended:?}"
    );
    assert_eq!(ended[0]["payload"]["outcome"], outcome);
    assert_eq!(ended[0]["payload"]["state"], state);
    assert_eq!(ended[0]["payload"]["reason"], closed_for().as_str());

    let read = status(world, print);
    assert!(
        read["print"]["ended_at"].is_string(),
        "the record was not ended: {read}"
    );
    assert_eq!(read["print"]["end_reason"], closed_for().as_str());
    assert!(
        read["session"]["closed_at"].is_string(),
        "the record's session was not closed: {read}"
    );
    assert_eq!(
        of_kind(world, print, "supervision_session_closed").len(),
        1,
        "the session's close is not in the history"
    );
    let listed = listed(world, print);
    assert_eq!(listed["ended_at"], read["print"]["ended_at"]);
    assert_eq!(listed["end_reason"], closed_for().as_str());
    assert!(
        of_kind(world, print, "filament_consumed").is_empty(),
        "the close charged the record"
    );
}

/// What every spool holds, by its identifier.
fn holdings(world: &World) -> Vec<(Value, Value)> {
    running::read(world, &["spools", "--include-retired", "true"])["spools"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|spool| (spool["id"].clone(), spool["remaining_g"].clone()))
        .collect()
}

/// A blank reason is refused before anything reaches the supervisor.
fn blank_reasons_are_refused(world: &World, left: &str) {
    for blank in ["", "   "] {
        let ran = close(world, left, "interrupted", blank);
        assert_eq!(
            ran.code,
            Some(i32::from(Exit::Usage.status())),
            "a close with the reason {blank:?} ran: {}",
            ran.said()
        );
        assert!(
            world.proxy.received().is_empty(),
            "a blank reason reached the server"
        );
        assert!(of_kind(world, left, "action_requested").is_empty());
        still_open(world, left);
    }
}

/// A job of the record's own file printing, paused or cancelling holds it
/// open, and the rejection names the state; so does any running job for a
/// record naming no file, which cannot be told apart from it.
fn a_running_job_holds_its_record_open(world: &World, running_one: &str) {
    for (state, word) in [
        (Reports::Printing, "printing"),
        (Reports::Paused, "paused"),
        (Reports::Printing, "cancelling"),
    ] {
        world.wants(state);
        let cancelling = word == "cancelling";
        world.machine_is_cancelling(cancelling);
        let ran = close(world, running_one, "cancelled", REASON);
        world.machine_is_cancelling(false);
        let refusal = rejected(world, running_one, &ran, "print_still_active");
        assert_eq!(refusal["state"], word, "{refusal}");
        still_open(world, running_one);
    }

    // A record naming no file cannot be told apart from the job the machine
    // runs, so that job holds it open.
    world.wants(Reports::Printing);
    let nameless = world.open_record_with_a_session(None, None);
    let ran = close(world, &nameless, "interrupted", REASON);
    let refusal = rejected(world, &nameless, &ran, "print_still_active");
    assert_eq!(refusal["state"], "printing", "{refusal}");
    still_open(world, &nameless);
}

/// A job of another file holds nothing open: the record closes, in the state
/// the machine reported running that job.
fn another_files_job_holds_nothing_open(world: &World) {
    world.wants(Reports::Printing);
    let stale = world.open_record_with_a_session(Some(STALE_FILE), None);
    let ran = close(world, &stale, "cancelled", REASON);
    closed(world, &stale, &ran, "cancelled", "printing");
}

/// A session already closed keeps the closure it has.
fn a_closed_session_keeps_its_closure(world: &World) {
    let watched_before = world.open_record_with_a_session(Some(STALE_FILE), Some(EARLIER_CLOSE));
    let session_before = status(world, &watched_before)["session"].clone();
    let ran = close(world, &watched_before, "cancelled", REASON);
    assert_eq!(
        ran.code,
        Some(0),
        "the close did not go through: {}",
        ran.said()
    );
    let after = status(world, &watched_before);
    assert!(
        after["print"]["ended_at"].is_string(),
        "the record was not ended: {after}"
    );
    assert_eq!(
        after["session"]["closed_at"], session_before["closed_at"],
        "{after}"
    );
    assert_eq!(after["session"]["close_reason"], EARLIER_CLOSE, "{after}");
}

/// An adjustment of the record still standing holds it open, naming it.
fn an_adjustment_holds_its_record_open(world: &World) {
    // The store files an action under the most recently opened open record,
    // so the record the adjustment is about is opened last.
    world.wants(Reports::Printing);
    let adjusted = world.open_record_with_a_session(Some(STALE_FILE), None);
    let adjusting = running::read(
        world,
        &[
            "set-fan-percent",
            "--print-id",
            &adjusted,
            "--percent",
            "35",
            "--duration-s",
            "3600",
            "--reason",
            "an adjustment of the record that still stands",
            "--actor",
            "operator",
        ],
    );
    let ran = close(world, &adjusted, "interrupted", REASON);
    let refusal = rejected(world, &adjusted, &ran, "intervention_still_active");
    assert_eq!(
        refusal["intervention_id"], adjusting["intervention"]["id"],
        "{refusal}"
    );
    still_open(world, &adjusted);
}

/// The whole journey, over one world, whose print it takes somewhere else: it
/// runs after every other journey of the tier.
///
/// # Panics
///
/// Panics at the first thing a close or a refusal did that it owed otherwise.
pub fn an_operator_closes_a_record_left_open(world: &World) {
    if !world.machine_is_unreachable(false) {
        // Only a socket can be told to be off, or to be cancelling.
        return;
    }
    world.wants(Reports::Printing);
    let left = world.open_record_with_a_session(Some(STALE_FILE), None);
    let running_one = world.open_record_with_a_session(Some(RUNNING_FILE), None);
    let spools = holdings(world);

    blank_reasons_are_refused(world, &left);
    a_running_job_holds_its_record_open(world, &running_one);
    another_files_job_holds_nothing_open(world);
    a_closed_session_keeps_its_closure(world);
    an_adjustment_holds_its_record_open(world);

    // A machine that does not answer refuses no close: the record ends in the
    // state it last knew.
    let last_known = status(world, &left)["print"]["state"].clone();
    assert!(world.machine_is_unreachable(true));
    let ran = close(world, &left, "interrupted", REASON);
    assert!(world.machine_is_unreachable(false));
    let last_known = last_known
        .as_str()
        .expect("a record's state is a word")
        .to_owned();
    closed(world, &left, &ran, "interrupted", &last_known);

    // A record already ended is refused as one, and keeps the end it has.
    let before = status(world, &left);
    let ended_before = of_kind(world, &left, "print_ended");
    let ran = close(world, &left, "completed", REASON);
    rejected(world, &left, &ran, "print_already_ended");
    let after = status(world, &left);
    assert_eq!(after["print"]["ended_at"], before["print"]["ended_at"]);
    assert_eq!(after["print"]["end_reason"], before["print"]["end_reason"]);
    assert_eq!(of_kind(world, &left, "print_ended"), ended_before);

    assert_eq!(holdings(world), spools, "a close debited a spool");

    // A machine showing no job closes a record in the state it reports. Every
    // read after it finds the machine idle and closes what else is open, so
    // this is the journey's last close.
    let idle = world.open_record_with_a_session(Some(STALE_FILE), None);
    world.wants(Reports::Operational);
    let ran = close(world, &idle, "failed", REASON);
    closed(world, &idle, &ran, "failed", "operational");

    // With no record left open, an ended one is still refused as ended.
    assert!(
        running::read(world, &["prints"])["prints"]
            .as_array()
            .into_iter()
            .flatten()
            .all(|print| print["ended_at"].is_string()),
        "a record is still open"
    );
    let ran = close(world, &idle, "completed", REASON);
    rejected(world, &idle, &ran, "print_already_ended");
    assert_eq!(of_kind(world, &idle, "print_ended").len(), 1);
}
