//! An operator closes a print record printobserver left open.
//!
//! These drive the close through the one action path, over a print this
//! supervisor started and charged as it ran, and read back what the record,
//! the history and the spool say afterwards: the print ends with the outcome
//! the operator gave, its consumption is charged nothing more, and nothing a
//! later read finds settles it a second time.

use printobserver_core::kinds::{
    FilamentConsumedPayload, PrintEndedPayload, UnaccountedPrintPayload,
};
use printobserver_core::store::{PrintStore as _, SessionStore as _, StoreError};
use printobserver_core::{
    Actor, ChargeRule, Clock as _, ExecutionOutcome, PolicyDecision, PrintAction, PrintOutcome,
    PrintRecord, RejectionReason, block_on, charge_rule,
};
use printobserver_printer_api::{PrinterError, PrinterState};
use printobserver_supervisor_api::SupervisionSession;
use printobserver_types::PrintId;

use crate::fakes::{PrinterMethod, StoreMethod};
use crate::shelf::{
    FILE, assert_close, charges_to, ends, every, every_of, loaded_spool, on_tool, running_at,
    spool_now, spool_world, start, tally_success,
};
use crate::world::World;

/// The reason every close in this walk gives.
const REASON: &str = "the printer was switched off mid-print";

/// The record of one print, as the store holds it now.
fn record(world: &World, print_id: PrintId) -> PrintRecord {
    block_on(world.store.print(print_id))
        .expect("the store reads")
        .expect("the print is recorded")
}

/// Close one print as the operator, with one outcome.
fn close(world: &World, print_id: PrintId, outcome: PrintOutcome) -> PolicyDecision {
    let answered = world
        .request(
            print_id,
            PrintAction::ClosePrint {
                outcome,
                reason: REASON.to_owned(),
                actor: Actor::Operator,
            },
        )
        .expect("the close is recorded");
    if answered.record.decision == PolicyDecision::Accepted {
        assert_eq!(
            answered.record.outcome,
            Some(ExecutionOutcome::Succeeded),
            "an accepted close was not carried out"
        );
    }
    answered.record.decision
}

/// A print this supervisor started and saw run, that the printer then stopped
/// without a read seeing it end: the stale record a close is for.
fn stale_print(world: &World) -> PrintId {
    let print_id = start(world);
    world
        .printer
        .extrudes(FILE, 200_000, vec![on_tool(0, 640.0)]);
    running_at(world, 200_000);
    world.printer.reports_state(PrinterState::Operational);
    world
        .printer
        .reports_job(Some(FILE), PrinterState::Operational);
    print_id
}

/// A close ends the record with the operator's outcome and the state the
/// printer reported, charges nothing, and is not settled again by a later read.
#[test]
fn a_close_ends_the_record_and_charges_nothing() {
    let world = spool_world();
    let spool = loaded_spool(&world, "Closed", 1_000.0);
    let print_id = stale_print(&world);
    let before = spool_now(&world, spool.id).remaining_g;

    assert_eq!(
        close(&world, print_id, PrintOutcome::Interrupted),
        PolicyDecision::Accepted
    );

    let closed = record(&world, print_id);
    assert!(closed.ended_at.is_some(), "the record was not ended");
    assert_eq!(
        closed.end_reason.as_deref(),
        Some(format!("closed by the operator: {REASON}").as_str())
    );
    let ended = every_of::<PrintEndedPayload>(&world, print_id);
    assert_eq!(ended.len(), 1, "one print_ended per print");
    assert_eq!(ended[0].outcome, PrintOutcome::Interrupted);
    assert_eq!(ended[0].state, PrinterState::Operational);
    assert_eq!(ended[0].reason, format!("closed by the operator: {REASON}"));
    assert_eq!(
        every_of::<FilamentConsumedPayload>(&world, print_id),
        Vec::new(),
        "a close charged the print"
    );
    assert_close(spool_now(&world, spool.id).remaining_g, before);

    // A read finding the job over afterwards settles nothing a second time.
    ends(&world, PrinterState::Operational);
    assert_eq!(every_of::<PrintEndedPayload>(&world, print_id).len(), 1);
    assert_eq!(
        every_of::<FilamentConsumedPayload>(&world, print_id),
        Vec::new()
    );
    assert_close(spool_now(&world, spool.id).remaining_g, before);
    assert_eq!(record(&world, print_id).ended_at, closed.ended_at);
}

/// The printer's own tally counting the print a close settled is no unseen
/// print: accounting for the prints this system never saw debits nothing for
/// it.
#[test]
fn a_closed_print_the_printers_tally_counts_is_not_charged_as_unseen() {
    let world = spool_world();
    let spool = loaded_spool(&world, "Tallied", 1_000.0);
    // The printer lists a file it has printed, so the baseline needs one.
    tally_success(&world, world.clock.now());
    block_on(world.core.account_unobserved_prints()).expect("the baseline is written");
    let print_id = stale_print(&world);
    let charged = charges_to(&world, spool.id).len();

    assert_eq!(
        close(&world, print_id, PrintOutcome::Completed),
        PolicyDecision::Accepted
    );
    tally_success(&world, record(&world, print_id).opened_at);
    block_on(world.core.account_unobserved_prints()).expect("the accounting reads");

    assert_eq!(every::<UnaccountedPrintPayload>(&world), Vec::new());
    assert_eq!(charges_to(&world, spool.id).len(), charged);
}

/// A printer that cannot be read refuses no close: the print ends in the state
/// its record last knew.
#[test]
fn a_close_with_the_printer_unreachable_ends_in_the_records_last_state() {
    let world = spool_world();
    let print_id = stale_print(&world);
    let last_known = record(&world, print_id).state;
    world.printer.fails(
        PrinterMethod::Job,
        PrinterError::Unreachable {
            detail: "the printer is off".to_owned(),
        },
    );

    assert_eq!(
        close(&world, print_id, PrintOutcome::Cancelled),
        PolicyDecision::Accepted
    );

    let ended = every_of::<PrintEndedPayload>(&world, print_id);
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].outcome, PrintOutcome::Cancelled);
    assert_eq!(ended[0].state, last_known);
    assert!(record(&world, print_id).ended_at.is_some());
}

/// An interrupted print is charged by the rule a failed one is: what the
/// file's moves extruded up to the last position seen. Only a completed print
/// is charged its whole analysed length.
#[test]
fn an_interrupted_print_is_charged_as_a_failed_one() {
    assert_eq!(
        charge_rule(PrintOutcome::Interrupted),
        charge_rule(PrintOutcome::Failed)
    );
    for (outcome, rule) in [
        (PrintOutcome::Completed, ChargeRule::SlicerEstimate),
        (PrintOutcome::Cancelled, ChargeRule::ExtrudedToLastPosition),
        (PrintOutcome::Failed, ChargeRule::ExtrudedToLastPosition),
        (
            PrintOutcome::Interrupted,
            ChargeRule::ExtrudedToLastPosition,
        ),
    ] {
        assert_eq!(charge_rule(outcome), rule, "{outcome:?}");
    }
}

/// A record that names no file cannot be told apart from whatever job the
/// printer holds, so any job printing holds it open; once the printer shows
/// nothing running it closes.
#[test]
fn a_record_naming_no_file_is_held_open_by_any_running_job() {
    let world = spool_world();
    let print = block_on(world.store.open_print(Some(31), None)).expect("the record opens");
    world.printer.reports_state(PrinterState::Printing);
    world
        .printer
        .reports_job(Some("another.gcode"), PrinterState::Printing);

    assert_eq!(
        close(&world, print.id, PrintOutcome::Interrupted),
        PolicyDecision::Rejected(RejectionReason::PrintStillActive {
            state: PrinterState::Printing,
        })
    );
    assert_eq!(record(&world, print.id).ended_at, None);

    world.printer.reports_state(PrinterState::Operational);
    world
        .printer
        .reports_job(Some("another.gcode"), PrinterState::Operational);
    assert_eq!(
        close(&world, print.id, PrintOutcome::Interrupted),
        PolicyDecision::Accepted
    );
    assert!(record(&world, print.id).ended_at.is_some());
}

/// A close whose end could not be written leaves the print settled and open,
/// and the next close only ends it: no second `print_ended`, and no charge.
#[test]
fn a_close_whose_end_failed_is_finished_by_the_next_close() {
    let world = spool_world();
    let spool = loaded_spool(&world, "Recovered", 1_000.0);
    let print_id = stale_print(&world);
    let before = spool_now(&world, spool.id).remaining_g;
    world.store.fails(
        StoreMethod::EndPrint,
        StoreError::Database {
            detail: "the disk went away".to_owned(),
        },
    );

    world
        .request(
            print_id,
            PrintAction::ClosePrint {
                outcome: PrintOutcome::Interrupted,
                reason: REASON.to_owned(),
                actor: Actor::Operator,
            },
        )
        .expect_err("the end could not be written");
    assert_eq!(record(&world, print_id).ended_at, None);
    assert_eq!(every_of::<PrintEndedPayload>(&world, print_id).len(), 1);

    world.store.heals();
    assert_eq!(
        close(&world, print_id, PrintOutcome::Interrupted),
        PolicyDecision::Accepted
    );

    assert!(record(&world, print_id).ended_at.is_some());
    let ended = every_of::<PrintEndedPayload>(&world, print_id);
    assert_eq!(ended.len(), 1, "the second close ended the print again");
    assert_eq!(ended[0].outcome, PrintOutcome::Interrupted);
    assert_eq!(
        every_of::<FilamentConsumedPayload>(&world, print_id),
        Vec::new()
    );
    assert_close(spool_now(&world, spool.id).remaining_g, before);
}

/// A close closes the print's open session in the record the store holds,
/// and leaves one already closed with the closure it has.
#[test]
fn a_close_closes_an_open_session_and_keeps_a_closed_ones_closure() {
    let world = spool_world();
    let open = stale_print(&world);
    block_on(world.store.put_session(session(open, None))).expect("the session is stored");
    assert_eq!(
        close(&world, open, PrintOutcome::Interrupted),
        PolicyDecision::Accepted
    );
    let closed = block_on(world.store.session(open))
        .expect("the store reads")
        .expect("the session is held");
    assert!(
        closed.closed_at.is_some(),
        "the open session was not closed"
    );
    assert_eq!(
        closed.close_reason.as_deref(),
        Some(format!("closed by the operator: {REASON}").as_str())
    );

    let earlier = world.clock.now();
    let already = block_on(world.store.open_print(Some(32), Some(FILE.to_owned())))
        .expect("the record opens");
    block_on(world.store.put_session(session(
        already.id,
        Some((earlier, "the harness closed it")),
    )))
    .expect("the session is stored");
    world.clock.advance(60);
    assert_eq!(
        close(&world, already.id, PrintOutcome::Cancelled),
        PolicyDecision::Accepted
    );
    let kept = block_on(world.store.session(already.id))
        .expect("the store reads")
        .expect("the session is held");
    assert_eq!(kept.closed_at, Some(earlier));
    assert_eq!(kept.close_reason.as_deref(), Some("the harness closed it"));
}

/// One print's supervision session, closed at an instant for a reason or
/// still open.
fn session(
    print_id: PrintId,
    closed: Option<(printobserver_types::Timestamp, &str)>,
) -> SupervisionSession {
    let at = printobserver_types::Timestamp::now();
    SupervisionSession {
        print_id,
        session_name: format!("print-{print_id}"),
        harness_identity: "claude-code".to_owned(),
        created_at: at,
        last_turn_at: at,
        closed_at: closed.map(|(when, _)| when),
        close_reason: closed.map(|(_, why)| why.to_owned()),
    }
}
