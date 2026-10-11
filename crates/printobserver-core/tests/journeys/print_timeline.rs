//! Every print is in the timeline from its start to its end, and charges the
//! spool on each tool it used by the rule its outcome names.
//!
//! These drive the supervisor the way the printer and a caller do — a start
//! through the action path, the job read back by the listing, a cancel asked
//! for or seen at the machine — and read what each print took back from the
//! timeline and the spool records.

use printobserver_core::kinds::{
    FilamentConsumedPayload, PrintEndedPayload, PrintStartedPayload, PrintUntrackedPayload,
    SpoolUnverifiedPayload,
};
use printobserver_core::store::StoreError;
use printobserver_core::{
    Actor, ChargeKey, ChargeMethod, Clock as _, LoadedSpool, PrintAction, PrintOutcome, SpoolId,
    SpoolTrust, UNTRACKED_REASON, UnverifiedReason, block_on,
};
use printobserver_printer_api::{FileFacts, PrinterState};
use printobserver_types::PrintId;

use crate::fakes::{PrinterMethod, StoreMethod};
use crate::shelf::{
    FILE, SIZE, TOTAL_MM, assert_close, assert_ledger_explains, charges_to, ends, ends_on, every,
    every_of, facts, grams, load, loaded_spool, on_tool, register, running_at, running_at_on,
    spool_now, spool_world, start, tally_success,
};
use crate::world::World;

/// The one `print_started` a print was opened with.
fn started_of(world: &World, print_id: PrintId) -> PrintStartedPayload {
    let started = every_of::<PrintStartedPayload>(world, print_id);
    assert_eq!(started.len(), 1, "one print_started per print");
    started.into_iter().next().expect("one")
}

/// The one `print_ended` a print was closed out with.
fn ended_of(world: &World, print_id: PrintId) -> PrintEndedPayload {
    let ended = every_of::<PrintEndedPayload>(world, print_id);
    assert_eq!(ended.len(), 1, "one print_ended per print");
    ended.into_iter().next().expect("one")
}

/// Every charge made for one print, oldest first.
fn charges_of(world: &World, print_id: PrintId) -> Vec<FilamentConsumedPayload> {
    every_of::<FilamentConsumedPayload>(world, print_id)
}

/// The print key of one segment of a print on tool 0.
const fn key(print_id: PrintId, segment: u32) -> ChargeKey {
    ChargeKey::Print {
        print_id,
        tool: 0,
        segment,
    }
}

/// Assert one charge is the one a spool's rule makes for a length.
#[track_caller]
fn assert_charge(
    charge: &FilamentConsumedPayload,
    spool_id: SpoolId,
    world: &World,
    method: ChargeMethod,
    length_mm: f64,
) {
    let spool = spool_now(world, spool_id);
    assert_eq!(charge.spool_id, Some(spool_id));
    assert_eq!(charge.method, method);
    assert_close(charge.length_mm, length_mm);
    assert_close(charge.grams.expect("weighed"), grams(&spool, length_mm));
    assert_eq!(charge.diameter_mm, Some(spool.diameter_mm));
    assert_eq!(charge.density_g_cm3, Some(spool.density_g_cm3));
    let before = charge.remaining_before_g.expect("a figure before");
    let after = charge.remaining_after_g.expect("a figure after");
    assert_close(after, before - grams(&spool, length_mm));
}

/// A completed print names its spool as it starts, ends `completed`, and
/// charges the spool the file's analysed length as a slicer estimate.
#[test]
fn a_completed_print_charges_the_analysed_length() {
    let world = spool_world();
    let spool = loaded_spool(&world, "Completed", 1_000.0);
    let print_id = start(&world);
    running_at(&world, SIZE / 2);
    running_at(&world, SIZE);
    ends(&world, PrinterState::Operational);

    let started = started_of(&world, print_id);
    assert_eq!(
        started.loaded,
        vec![LoadedSpool {
            tool: 0,
            spool_id: spool.id
        }]
    );
    assert!(started.started_by.is_some(), "a start names its action");
    assert_eq!(started.file_name.as_deref(), Some(FILE));
    assert!(started.preflight.is_some(), "a start carries its check");
    assert_eq!(ended_of(&world, print_id).outcome, PrintOutcome::Completed);
    let charges = charges_of(&world, print_id);
    assert_eq!(charges.len(), 1);
    assert_charge(
        &charges[0],
        spool.id,
        &world,
        ChargeMethod::SlicerEstimate,
        TOTAL_MM,
    );
    assert_eq!(charges[0].charge_key, key(print_id, 0));
    assert_eq!(charges[0].analysed_length_mm, Some(TOTAL_MM));
    assert_close(
        spool_now(&world, spool.id).remaining_g,
        1_000.0 - grams(&spool, TOTAL_MM),
    );
    assert_ledger_explains(&world, spool.id);
    world.journal.assert_no_violations();
}

/// A print cancelled through this system reads where it had got before the
/// printer is asked to cancel — `OctoPrint` clears the position as it does —
/// ends `cancelled`, and charges the extrusion the file's moves add up to at
/// that position.
#[test]
fn a_print_cancelled_here_charges_the_extrusion_to_where_it_stopped() {
    let world = spool_world();
    let spool = loaded_spool(&world, "Cancelled Here", 1_000.0);
    let print_id = start(&world);
    running_at(&world, 400_000);
    world
        .printer
        .extrudes(FILE, 450_000, vec![on_tool(0, 2_310.5)]);
    world.printer.reports_position(Some(450_000), Some(SIZE));
    let outcome = world
        .request(
            print_id,
            PrintAction::Cancel {
                reason: "the first layer lifted".to_owned(),
                actor: Actor::Operator,
            },
        )
        .expect("the cancel is recorded");
    assert_eq!(outcome.executed, Some(Ok(())));
    ends(&world, PrinterState::Operational);

    assert_eq!(started_of(&world, print_id).loaded[0].spool_id, spool.id);
    let ended = ended_of(&world, print_id);
    assert_eq!(ended.outcome, PrintOutcome::Cancelled);
    assert_eq!(
        ended.last_position.as_ref().map(|position| position.bytes),
        Some(450_000)
    );
    let charges = charges_of(&world, print_id);
    assert_eq!(charges.len(), 1);
    assert_charge(
        &charges[0],
        spool.id,
        &world,
        ChargeMethod::GcodeExtrusion,
        2_310.5,
    );
    assert_eq!(
        charges[0].position.as_ref().map(|position| position.bytes),
        Some(450_000)
    );
    assert_ledger_explains(&world, spool.id);
}

/// A print this system cancelled is `cancelled` whatever state the printer
/// reports after it — an error the cancel itself caused included — because it
/// was this system's own cancel that ended it.
#[test]
fn a_print_cancelled_here_stays_cancelled_whatever_state_follows() {
    let world = spool_world();
    let spool = loaded_spool(&world, "Cancelled Then Error", 1_000.0);
    let print_id = start(&world);
    running_at(&world, 250_000);
    world
        .printer
        .extrudes(FILE, 250_000, vec![on_tool(0, 1_200.0)]);
    let outcome = world
        .request(
            print_id,
            PrintAction::Cancel {
                reason: "the nozzle is dragging".to_owned(),
                actor: Actor::Operator,
            },
        )
        .expect("the cancel is recorded");
    assert_eq!(outcome.executed, Some(Ok(())));
    ends(&world, PrinterState::Error);

    assert_eq!(ended_of(&world, print_id).outcome, PrintOutcome::Cancelled);
    assert_charge(
        &charges_of(&world, print_id)[0],
        spool.id,
        &world,
        ChargeMethod::GcodeExtrusion,
        1_200.0,
    );
}

/// A print the printer was seen cancelling ends `cancelled` and charges the
/// extrusion up to the last position a read recorded.
#[test]
fn a_print_cancelled_at_the_printer_charges_the_extrusion_to_the_last_read() {
    let world = spool_world();
    let spool = loaded_spool(&world, "Cancelled There", 1_000.0);
    let print_id = start(&world);
    running_at(&world, 300_000);
    world
        .printer
        .extrudes(FILE, 300_000, vec![on_tool(0, 1_480.25)]);
    ends(&world, PrinterState::Cancelling);

    assert_eq!(started_of(&world, print_id).loaded[0].spool_id, spool.id);
    assert_eq!(ended_of(&world, print_id).outcome, PrintOutcome::Cancelled);
    let charges = charges_of(&world, print_id);
    assert_eq!(charges.len(), 1);
    assert_charge(
        &charges[0],
        spool.id,
        &world,
        ChargeMethod::GcodeExtrusion,
        1_480.25,
    );
    assert_ledger_explains(&world, spool.id);
}

/// A look the agent takes while the print runs records where it had got, so a
/// print cancelled at the printer after it is charged to that position rather
/// than to an older one.
#[test]
fn a_look_records_where_the_print_had_got() {
    let world = spool_world();
    let spool = loaded_spool(&world, "Looked At", 1_000.0);
    let print_id = start(&world);
    running_at(&world, 100_000);
    world
        .printer
        .extrudes(FILE, 100_000, vec![on_tool(0, 480.0)]);
    world
        .printer
        .extrudes(FILE, 700_000, vec![on_tool(0, 3_420.0)]);
    world.printer.reports_position(Some(700_000), Some(SIZE));
    block_on(
        world
            .core
            .take_look(print_id, std::time::Duration::ZERO, Vec::new()),
    )
    .expect("the look is taken");
    ends(&world, PrinterState::Cancelling);
    assert_charge(
        &charges_of(&world, print_id)[0],
        spool.id,
        &world,
        ChargeMethod::GcodeExtrusion,
        3_420.0,
    );
}

/// A print that ended in error ends `failed`; when the file itself cannot be
/// read, it is charged the analysed length scaled by how far in it got, with a
/// note saying why.
#[test]
fn a_failed_print_whose_file_cannot_be_read_charges_a_file_fraction() {
    let world = spool_world();
    let spool = loaded_spool(&world, "Failed", 1_000.0);
    let print_id = start(&world);
    running_at(&world, 600_000);
    world.printer.fails(
        PrinterMethod::ExtrudedTo,
        printobserver_printer_api::PrinterError::Unreachable {
            detail: "the file is gone".to_owned(),
        },
    );
    ends(&world, PrinterState::Error);

    assert_eq!(ended_of(&world, print_id).outcome, PrintOutcome::Failed);
    let charges = charges_of(&world, print_id);
    assert_eq!(charges.len(), 1);
    assert_charge(
        &charges[0],
        spool.id,
        &world,
        ChargeMethod::FileFraction,
        TOTAL_MM * 0.6,
    );
    let note = charges[0].note.as_deref().expect("a fraction says why");
    assert!(note.contains("could not be read"), "{note}");
    assert!(note.contains("the file is gone"), "{note}");
    assert_ledger_explains(&world, spool.id);
}

/// A print that ended offline ends `failed` and charges the extrusion to the
/// last position read.
#[test]
fn a_print_that_went_offline_fails_and_charges_the_extrusion() {
    let world = spool_world();
    let spool = loaded_spool(&world, "Offline", 1_000.0);
    let print_id = start(&world);
    running_at(&world, 100_000);
    world
        .printer
        .extrudes(FILE, 100_000, vec![on_tool(0, 512.0)]);
    ends(&world, PrinterState::Offline);
    assert_eq!(ended_of(&world, print_id).outcome, PrintOutcome::Failed);
    assert_charge(
        &charges_of(&world, print_id)[0],
        spool.id,
        &world,
        ChargeMethod::GcodeExtrusion,
        512.0,
    );
}

/// A job somebody started at the printer is found by a read: its print starts
/// with no `started_by` and names the spool loaded then, and its close-out
/// charges that spool by its outcome's rule.
#[test]
fn a_job_started_at_the_printer_is_adopted_and_charged() {
    let world = spool_world();
    let spool = loaded_spool(&world, "Adopted", 1_000.0);
    running_at(&world, 200_000);
    let listing = block_on(world.core.list_and_adopt_prints()).expect("listed");
    let print_id = listing.active.expect("the job was adopted");
    running_at(&world, SIZE);
    ends(&world, PrinterState::Operational);

    let started = started_of(&world, print_id);
    assert_eq!(started.started_by, None);
    assert_eq!(started.preflight, None);
    assert_eq!(
        started.loaded,
        vec![LoadedSpool {
            tool: 0,
            spool_id: spool.id
        }]
    );
    assert_eq!(ended_of(&world, print_id).outcome, PrintOutcome::Completed);
    let charges = charges_of(&world, print_id);
    assert_eq!(charges.len(), 1);
    assert_charge(
        &charges[0],
        spool.id,
        &world,
        ChargeMethod::SlicerEstimate,
        TOTAL_MM,
    );
    assert_ledger_explains(&world, spool.id);
}

/// A spool swapped mid-print charges the outgoing spool up to the swap and the
/// incoming one only the remainder, the two summing to the tool's total.
#[test]
fn a_mid_print_swap_splits_the_print_between_the_two_spools() {
    let world = spool_world();
    let outgoing = loaded_spool(&world, "Outgoing", 1_000.0);
    let incoming = register(&world, "Incoming", 1_000.0);
    let print_id = start(&world);
    running_at(&world, 400_000);
    world
        .printer
        .extrudes(FILE, 400_000, vec![on_tool(0, 2_000.0)]);
    let swap = load(&world, incoming.id, 0);
    assert_eq!(swap.decision, printobserver_core::PolicyDecision::Accepted);
    running_at(&world, SIZE);
    ends(&world, PrinterState::Operational);

    let out = charges_to(&world, outgoing.id);
    let into = charges_to(&world, incoming.id);
    assert_eq!(out.len(), 1);
    assert_eq!(into.len(), 1);
    assert_charge(
        &out[0],
        outgoing.id,
        &world,
        ChargeMethod::GcodeExtrusion,
        2_000.0,
    );
    assert_eq!(out[0].charge_key, key(print_id, 0));
    assert_charge(
        &into[0],
        incoming.id,
        &world,
        ChargeMethod::SlicerEstimate,
        TOTAL_MM - 2_000.0,
    );
    assert_eq!(into[0].charge_key, key(print_id, 1));
    assert_close(out[0].length_mm + into[0].length_mm, TOTAL_MM);
    assert_ledger_explains(&world, outgoing.id);
    assert_ledger_explains(&world, incoming.id);
}

/// A tool the print used with no tracked spool on it is recorded uncharged,
/// beside the charge to the spool on the tool that has one.
#[test]
fn a_tool_with_no_tracked_spool_is_recorded_uncharged() {
    let world = spool_world();
    world.printer.holds_file(FileFacts {
        filament: Some(vec![on_tool(0, 3_000.0), on_tool(1, 2_000.0)]),
        ..facts(None)
    });
    let spool = loaded_spool(&world, "Tool Zero", 1_000.0);
    let print_id = start(&world);
    running_at(&world, SIZE);
    ends(&world, PrinterState::Operational);

    let charges = charges_of(&world, print_id);
    assert_eq!(charges.len(), 2);
    let tracked = charges
        .iter()
        .find(|charge| charge.tool == 0)
        .expect("tool 0");
    assert_charge(
        tracked,
        spool.id,
        &world,
        ChargeMethod::SlicerEstimate,
        3_000.0,
    );
    let untracked = charges
        .iter()
        .find(|charge| charge.tool == 1)
        .expect("tool 1");
    assert_eq!(untracked.spool_id, None);
    assert_eq!(untracked.grams, None);
    assert_eq!(untracked.remaining_before_g, None);
    assert_close(untracked.length_mm, 2_000.0);
}

/// A print of a file with no analysis and no readable moves — an SD-card
/// file — charges nothing and marks the spool that fed it as one whose figure
/// is in doubt, saying which print.
#[test]
fn an_sd_card_print_marks_the_loaded_spool_uncharged() {
    let world = spool_world();
    world.printer.holds_file(FileFacts {
        origin: "sdcard".to_owned(),
        ..facts(None)
    });
    let spool = loaded_spool(&world, "SD Card", 1_000.0);
    running_at(&world, 10_000);
    let print_id = block_on(world.core.list_and_adopt_prints())
        .expect("listed")
        .active
        .expect("adopted");
    running_at(&world, SIZE);
    ends(&world, PrinterState::Operational);

    assert_eq!(ended_of(&world, print_id).outcome, PrintOutcome::Completed);
    assert!(charges_of(&world, print_id).is_empty());
    let marked = every::<SpoolUnverifiedPayload>(&world);
    assert_eq!(marked.len(), 1);
    assert_eq!(marked[0].spool_id, spool.id);
    assert_eq!(
        marked[0].reason,
        UnverifiedReason::UnchargedPrint { print_id }
    );
    let SpoolTrust::Unverified { marks } = spool_now(&world, spool.id).trust else {
        panic!("the spool is unverified");
    };
    assert_eq!(marks.len(), 1);
    assert_close(spool_now(&world, spool.id).remaining_g, 1_000.0);
}

/// A close-out repeated for a print already settled — its end made to fail
/// once — writes no second end or charge and debits nothing more: it only
/// ends the print.
#[test]
fn a_close_out_repeated_after_settling_only_ends_the_print() {
    let world = spool_world();
    let spool = loaded_spool(&world, "Replayed", 1_000.0);
    let print_id = start(&world);
    running_at(&world, SIZE);
    world.store.fails(
        StoreMethod::EndPrint,
        StoreError::Database {
            detail: "the disk went away".to_owned(),
        },
    );
    world
        .printer
        .reports_job(Some(FILE), PrinterState::Operational);
    block_on(world.core.list_and_adopt_prints()).expect_err("the end could not be written");
    assert_eq!(
        world.store.print_now(print_id).expect("held").ended_at,
        None
    );
    let settled_at = spool_now(&world, spool.id).remaining_g;

    world.store.heals();
    ends(&world, PrinterState::Operational);

    assert!(
        world
            .store
            .print_now(print_id)
            .expect("held")
            .ended_at
            .is_some()
    );
    assert_eq!(every_of::<PrintEndedPayload>(&world, print_id).len(), 1);
    assert_eq!(charges_of(&world, print_id).len(), 1);
    assert_close(spool_now(&world, spool.id).remaining_g, settled_at);
    assert_ledger_explains(&world, spool.id);
}

/// The same file printed twice in a row is two prints: each settled and
/// charged once under its own keys, and the file's tally growing by two is two
/// prints this system saw rather than two it never did.
#[test]
fn the_same_file_printed_twice_is_two_prints_each_charged_once() {
    let world = spool_world();
    let spool = loaded_spool(&world, "Twice", 1_000.0);
    assert!(
        block_on(world.core.account_unobserved_prints())
            .expect("the baseline is stored")
            .is_empty()
    );
    let mut prints = Vec::new();
    for _ in 0..2 {
        let print_id = start(&world);
        running_at(&world, SIZE);
        ends(&world, PrinterState::Operational);
        tally_success(&world, world.clock.now());
        prints.push(print_id);
    }
    assert_ne!(prints[0], prints[1]);
    for print_id in &prints {
        assert_eq!(ended_of(&world, *print_id).outcome, PrintOutcome::Completed);
        let charges = charges_of(&world, *print_id);
        assert_eq!(charges.len(), 1);
        assert_eq!(charges[0].charge_key, key(*print_id, 0));
    }
    assert_close(
        spool_now(&world, spool.id).remaining_g,
        1_000.0 - 2.0 * grams(&spool, TOTAL_MM),
    );
    let accounted = block_on(world.core.account_unobserved_prints()).expect("accounted");
    assert!(
        accounted.is_empty(),
        "two prints this system settled were taken for prints it never saw: {accounted:?}"
    );
    assert_ledger_explains(&world, spool.id);
}

/// A print already open when spool tracking was installed — it has no
/// `print_started` — ends with one `print_untracked`, and charges nothing.
#[test]
fn a_print_from_before_spool_tracking_ends_untracked() {
    let world = spool_world();
    let spool = loaded_spool(&world, "Untouched", 1_000.0);
    let print = world.open_print(7);
    running_at(&world, SIZE / 2);
    ends(&world, PrinterState::Operational);
    ends(&world, PrinterState::Operational);

    let untracked = every_of::<PrintUntrackedPayload>(&world, print.id);
    assert_eq!(untracked.len(), 1);
    assert_eq!(untracked[0].reason, UNTRACKED_REASON);
    assert!(charges_of(&world, print.id).is_empty());
    assert!(every_of::<PrintStartedPayload>(&world, print.id).is_empty());
    let unmoved = spool_now(&world, spool.id);
    assert_close(unmoved.remaining_g, 1_000.0);
    assert_eq!(unmoved.trust, SpoolTrust::Verified);
}

/// A print that completed while nothing was watching is settled once, on the
/// first read after the restart, charged in full when the file's tally shows a
/// successful print since it began.
#[test]
fn a_print_that_completed_during_downtime_is_settled_once_after_a_restart() {
    let world = spool_world();
    let spool = loaded_spool(&world, "Down Completed", 1_000.0);
    let print_id = start(&world);
    running_at(&world, 400_000);
    tally_success(&world, world.clock.now());
    let restarted = world.restarted();
    ends_on(&world, &restarted, PrinterState::Operational);
    ends_on(&world, &restarted, PrinterState::Operational);

    assert_eq!(ended_of(&world, print_id).outcome, PrintOutcome::Completed);
    let charges = charges_of(&world, print_id);
    assert_eq!(charges.len(), 1);
    assert_charge(
        &charges[0],
        spool.id,
        &world,
        ChargeMethod::SlicerEstimate,
        TOTAL_MM,
    );
    assert_eq!(spool_now(&world, spool.id).trust, SpoolTrust::Verified);
}

/// A print that stopped unfinished while nothing was watching is charged the
/// extrusion to the last position recorded before the downtime, and its spool
/// is marked, because what it fed after that position is unknown.
#[test]
fn a_print_that_stopped_during_downtime_is_charged_to_its_last_position_and_marked() {
    let world = spool_world();
    let spool = loaded_spool(&world, "Down Stopped", 1_000.0);
    let print_id = start(&world);
    running_at(&world, 400_000);
    world
        .printer
        .extrudes(FILE, 400_000, vec![on_tool(0, 1_900.0)]);
    let restarted = world.restarted();
    ends_on(&world, &restarted, PrinterState::Operational);
    ends_on(&world, &restarted, PrinterState::Operational);

    assert_eq!(ended_of(&world, print_id).outcome, PrintOutcome::Cancelled);
    let charges = charges_of(&world, print_id);
    assert_eq!(charges.len(), 1);
    assert_charge(
        &charges[0],
        spool.id,
        &world,
        ChargeMethod::GcodeExtrusion,
        1_900.0,
    );
    let SpoolTrust::Unverified { marks } = spool_now(&world, spool.id).trust else {
        panic!("the spool is unverified");
    };
    assert_eq!(
        marks
            .iter()
            .map(|mark| mark.reason.clone())
            .collect::<Vec<_>>(),
        vec![UnverifiedReason::UnchargedPrint { print_id }]
    );
    assert_eq!(every::<SpoolUnverifiedPayload>(&world).len(), 1);
}

/// A supervisor built after a mid-print swap charges the incoming spool only
/// the tool's total less the segment the outgoing spool was already charged,
/// so the print's total is charged exactly once.
#[test]
fn a_restart_after_a_mid_print_swap_charges_only_the_remainder() {
    let world = spool_world();
    let outgoing = loaded_spool(&world, "Before Restart", 1_000.0);
    let incoming = register(&world, "After Restart", 1_000.0);
    let print_id = start(&world);
    running_at(&world, 400_000);
    world
        .printer
        .extrudes(FILE, 400_000, vec![on_tool(0, 2_000.0)]);
    assert_eq!(
        load(&world, incoming.id, 0).decision,
        printobserver_core::PolicyDecision::Accepted
    );

    let restarted = world.restarted();
    running_at_on(&world, &restarted, SIZE);
    ends_on(&world, &restarted, PrinterState::Operational);

    let into = charges_to(&world, incoming.id);
    assert_eq!(into.len(), 1);
    assert_close(into[0].length_mm, TOTAL_MM - 2_000.0);
    assert_eq!(into[0].charge_key, key(print_id, 1));
    let total: f64 = charges_of(&world, print_id)
        .iter()
        .map(|charge| charge.length_mm)
        .sum();
    assert_close(total, TOTAL_MM);
    assert_ledger_explains(&world, outgoing.id);
    assert_ledger_explains(&world, incoming.id);
}

/// A close-out of a print from before spool tracking, replayed after its
/// settlement was refused — or committed with its answer lost — still writes
/// exactly one `print_untracked`, and the print ends.
#[test]
fn an_untracked_close_out_replayed_writes_one_print_untracked() {
    for lost in [false, true] {
        let world = spool_world();
        let print = world.open_print(7);
        running_at(&world, SIZE / 2);
        let error = StoreError::Database {
            detail: "the connection dropped".to_owned(),
        };
        if lost {
            world
                .store
                .loses_the_answer_to(StoreMethod::SettlePrint, error);
        } else {
            world.store.fails(StoreMethod::SettlePrint, error);
        }
        world
            .printer
            .reports_job(Some(FILE), PrinterState::Operational);
        block_on(world.core.list_and_adopt_prints()).expect_err("the settlement failed");
        world.store.heals();
        ends(&world, PrinterState::Operational);

        assert_eq!(
            every_of::<PrintUntrackedPayload>(&world, print.id).len(),
            1,
            "lost={lost}"
        );
        assert_eq!(every_of::<PrintEndedPayload>(&world, print.id).len(), 1);
        assert!(
            world
                .store
                .print_now(print.id)
                .expect("held")
                .ended_at
                .is_some()
        );
    }
}

/// A look whose position the store will not keep is still answered, and the
/// next read that finds the print running records where it had got.
#[test]
fn a_look_whose_position_is_refused_is_answered_and_a_later_read_records_it() {
    let world = spool_world();
    let spool = loaded_spool(&world, "Looked At Twice", 1_000.0);
    let print_id = start(&world);
    world
        .printer
        .extrudes(FILE, 500_000, vec![on_tool(0, 2_500.0)]);
    world.printer.reports_position(Some(500_000), Some(SIZE));
    world.store.fails(
        StoreMethod::RecordFilePosition,
        StoreError::Database {
            detail: "the disk is full".to_owned(),
        },
    );
    let look = block_on(
        world
            .core
            .take_look(print_id, std::time::Duration::ZERO, Vec::new()),
    )
    .expect("the look is answered");
    assert_eq!(
        look.job.and_then(|job| job.file_position_bytes),
        Some(500_000)
    );
    world.store.heals();
    running_at(&world, 500_000);
    ends(&world, PrinterState::Cancelling);
    assert_charge(
        &charges_of(&world, print_id)[0],
        spool.id,
        &world,
        ChargeMethod::GcodeExtrusion,
        2_500.0,
    );
}

/// A print whose job reports itself complete as it ends is completed, though
/// its last position fell short of the file's end and no tally says so; a
/// completion reported for another file's job is not this print's.
#[test]
fn a_job_reporting_its_own_completion_completes_the_print() {
    for same_file in [true, false] {
        let world = spool_world();
        let spool = loaded_spool(&world, "Reported Complete", 1_000.0);
        let print_id = start(&world);
        running_at(&world, 990_000);
        world
            .printer
            .extrudes(FILE, 990_000, vec![on_tool(0, 4_950.0)]);
        world.clock.advance(60);
        world.printer.reports_completion(Some(1.0));
        world.printer.reports_state(PrinterState::Operational);
        let file = if same_file { FILE } else { "calibration.gcode" };
        world
            .printer
            .reports_job(Some(file), PrinterState::Operational);
        world.printer.reports_position(None, Some(SIZE));
        block_on(world.core.list_and_adopt_prints()).expect("listed");

        let ended = ended_of(&world, print_id);
        let charge = &charges_of(&world, print_id)[0];
        if same_file {
            assert_eq!(ended.outcome, PrintOutcome::Completed);
            assert_charge(
                charge,
                spool.id,
                &world,
                ChargeMethod::SlicerEstimate,
                TOTAL_MM,
            );
        } else {
            assert_eq!(ended.outcome, PrintOutcome::Cancelled);
            assert_charge(
                charge,
                spool.id,
                &world,
                ChargeMethod::GcodeExtrusion,
                4_950.0,
            );
        }
    }
}

/// A position a printer reports before the file's start is no position: it is
/// not recorded, and the print is charged to the last real one.
#[test]
fn a_position_before_the_file_s_start_is_not_recorded() {
    let world = spool_world();
    let spool = loaded_spool(&world, "Odd Position", 1_000.0);
    let print_id = start(&world);
    running_at(&world, 300_000);
    world
        .printer
        .extrudes(FILE, 300_000, vec![on_tool(0, 1_400.0)]);
    running_at(&world, -5);
    ends(&world, PrinterState::Cancelling);
    assert_charge(
        &charges_of(&world, print_id)[0],
        spool.id,
        &world,
        ChargeMethod::GcodeExtrusion,
        1_400.0,
    );
}

/// A print first opened by an alert — no print open, a spool loaded — is
/// opened with the alert itself written first and exactly one `print_started`
/// straight after it, naming no start and the spool loaded; and it is charged
/// like any other print when it ends, never written off as untracked.
#[test]
fn a_print_an_alert_opens_is_started_charged_and_never_untracked() {
    let world = spool_world();
    let spool = loaded_spool(&world, "Alert Opened", 1_000.0);
    assert!(world.store.prints().is_empty(), "a print was already open");

    let event = world
        .handle(crate::world::failure_alert(7))
        .expect("the alert is handled");
    let print_id = event.print_id.expect("the alert opened a print");

    let history = world.store.events_of(print_id);
    let at = history
        .iter()
        .position(|record| record.id == event.id)
        .expect("the alert is in its print's history");
    let next = history.get(at + 1).expect("something follows the alert");
    let started = next
        .payload_as::<PrintStartedPayload>()
        .expect("the alert is followed by its print_started")
        .expect("a print_started reads as one");
    assert_eq!(
        history
            .iter()
            .filter(|record| record.body.is::<PrintStartedPayload>())
            .count(),
        1,
        "the alert's print was started more than once"
    );
    assert_eq!(started.started_by, None);
    assert_eq!(started.preflight, None);
    assert_eq!(started.file_name.as_deref(), Some(FILE));
    assert_eq!(
        started.loaded,
        vec![LoadedSpool {
            tool: 0,
            spool_id: spool.id
        }]
    );

    running_at(&world, SIZE);
    ends(&world, PrinterState::Operational);

    assert_eq!(ended_of(&world, print_id).outcome, PrintOutcome::Completed);
    let charges = charges_of(&world, print_id);
    assert_eq!(charges.len(), 1);
    assert_charge(
        &charges[0],
        spool.id,
        &world,
        ChargeMethod::SlicerEstimate,
        TOTAL_MM,
    );
    assert_eq!(charges[0].charge_key, key(print_id, 0));
    assert!(every_of::<PrintUntrackedPayload>(&world, print_id).is_empty());
    assert_close(
        spool_now(&world, spool.id).remaining_g,
        1_000.0 - grams(&spool, TOTAL_MM),
    );
    assert_ledger_explains(&world, spool.id);
    world.journal.assert_no_violations();
}
