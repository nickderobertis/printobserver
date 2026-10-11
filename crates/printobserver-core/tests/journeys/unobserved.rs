//! The prints the printer's own tallies counted and this system never saw are
//! accounted, once each.
//!
//! These move the printer's per-file tallies the way `OctoPrint` does — a
//! print completing or failing, a file deleted and uploaded again — and run the
//! accounting the server runs as it starts, reading back what it recorded.

use printobserver_core::kinds::{
    FileTallyRebaselinedPayload, RebaselineDisposition, UnaccountedPrintPayload,
};
use printobserver_core::store::StoreError;
use printobserver_core::{
    ChargeKey, ChargeMethod, Clock as _, SpoolTrust, UnverifiedReason, block_on,
};
use printobserver_printer_api::{FilePrint, PrinterState};
use printobserver_types::EventRecord;

use crate::fakes::StoreMethod;
use crate::shelf::{
    FILE, SIZE, TOTAL_MM, assert_close, assert_ledger_explains, charges_to, ends, every, grams,
    loaded_spool, running_at, spool_now, spool_world, start, tally_failure, tally_success,
};
use crate::world::World;

/// Run the accounting the server runs as it starts.
fn account(world: &World) -> Vec<EventRecord> {
    world.clock.advance(60);
    block_on(world.core.account_unobserved_prints()).expect("the accounting is written")
}

/// Set the file's tally outright, as a re-upload leaves it.
fn tally(world: &World, successes: i64, failures: i64, last_success: bool) {
    let mut held = world.printer.file(FILE);
    held.successes = successes;
    held.failures = failures;
    held.last_print = Some(FilePrint {
        at: world.clock.now(),
        success: last_success,
    });
    world.printer.holds_file(held);
}

/// The key one unseen print of the file charges on tool 0.
fn unobserved(generation: u32, print_number: i64) -> ChargeKey {
    ChargeKey::Unobserved {
        origin: "local".to_owned(),
        file_name: FILE.to_owned(),
        generation,
        print_number,
        tool: 0,
    }
}

/// The first run stores the baseline and flags nothing; prints the tallies
/// then show this system never closed are accounted — a completed one charged
/// its analysed length, a failed one marking the loaded spool — and a print it
/// did close is not; an unchanged run writes nothing, and a later one accounts
/// only for what is new.
#[test]
fn unseen_prints_are_accounted_once_and_seen_ones_never() {
    let world = spool_world();
    let spool = loaded_spool(&world, "Tallied", 1_000.0);
    tally(&world, 3, 1, true);
    assert!(
        account(&world).is_empty(),
        "the first run flagged something"
    );
    assert!(every::<UnaccountedPrintPayload>(&world).is_empty());

    // A print this system ran and settled is not one it never saw.
    start(&world);
    running_at(&world, SIZE);
    ends(&world, PrinterState::Operational);
    tally_success(&world, world.clock.now());
    assert!(
        account(&world).is_empty(),
        "a print this system closed was flagged"
    );

    tally_success(&world, world.clock.now());
    let printed_at = world.clock.now();
    tally_failure(&world, printed_at);
    let recorded = account(&world);
    let unaccounted = every::<UnaccountedPrintPayload>(&world);
    assert_eq!(unaccounted.len(), 2);
    assert!(unaccounted[0].success && unaccounted[0].charged);
    assert!(!unaccounted[1].success && !unaccounted[1].charged);
    assert_eq!(
        recorded.len(),
        4,
        "a charge, a mark and two prints: {recorded:?}"
    );
    let charges: Vec<_> = charges_to(&world, spool.id)
        .into_iter()
        .filter(|charge| charge.method == ChargeMethod::UnobservedEstimate)
        .collect();
    assert_eq!(charges.len(), 1);
    assert_eq!(charges[0].charge_key, unobserved(0, 6));
    assert_close(charges[0].length_mm, TOTAL_MM);
    assert_close(charges[0].grams.expect("weighed"), grams(&spool, TOTAL_MM));
    let SpoolTrust::Unverified { marks } = spool_now(&world, spool.id).trust else {
        panic!("the failed print left no mark");
    };
    assert_eq!(
        marks[0].reason,
        UnverifiedReason::UnaccountedPrint {
            file_name: FILE.to_owned(),
            printed_at,
        }
    );

    assert!(
        account(&world).is_empty(),
        "an unchanged run wrote something"
    );
    assert_eq!(charges_to(&world, spool.id).len(), 2);

    tally_success(&world, world.clock.now());
    account(&world);
    assert_eq!(every::<UnaccountedPrintPayload>(&world).len(), 3);
    let charges = charges_to(&world, spool.id);
    assert_eq!(
        charges
            .iter()
            .filter(|charge| charge.method == ChargeMethod::UnobservedEstimate)
            .count(),
        2
    );
    assert_eq!(
        charges.last().expect("charged").charge_key,
        unobserved(0, 8)
    );
    assert_ledger_explains(&world, spool.id);
}

/// A file whose tally went down advances its generation with one rebaseline
/// naming both: with nothing of it settled since the previous tally, the new
/// count's prints are charged under the new generation's keys and no spool is
/// marked; with a print of it settled in that window, nothing is charged on a
/// guess and the loaded spool is marked.
#[test]
fn a_tally_that_went_down_is_rebaselined_and_accounted_or_marked() {
    for settled_in_window in [false, true] {
        let world = spool_world();
        let spool = loaded_spool(&world, "Re-uploaded", 1_000.0);
        tally(&world, 4, 1, true);
        account(&world);
        if settled_in_window {
            start(&world);
            running_at(&world, SIZE);
            ends(&world, PrinterState::Operational);
        }
        tally(&world, 1, 0, true);
        account(&world);

        let rebaselined = every::<FileTallyRebaselinedPayload>(&world);
        assert_eq!(rebaselined.len(), 1);
        let event = &rebaselined[0];
        assert_eq!((event.old_generation, event.new_generation), (0, 1));
        assert_eq!((event.old_successes, event.old_failures), (4, 1));
        assert_eq!((event.new_successes, event.new_failures), (1, 0));
        let unobserved_charges: Vec<_> = charges_to(&world, spool.id)
            .into_iter()
            .filter(|charge| charge.method == ChargeMethod::UnobservedEstimate)
            .collect();
        if settled_in_window {
            assert_eq!(event.disposition, RebaselineDisposition::Unverified);
            assert!(unobserved_charges.is_empty());
            let SpoolTrust::Unverified { marks } = spool_now(&world, spool.id).trust else {
                panic!("the reset left no mark");
            };
            assert_eq!(
                marks[0].reason,
                UnverifiedReason::TallyReset {
                    file_name: FILE.to_owned(),
                    origin: "local".to_owned(),
                }
            );
        } else {
            assert_eq!(event.disposition, RebaselineDisposition::Accounted);
            assert_eq!(unobserved_charges.len(), 1);
            assert_eq!(unobserved_charges[0].charge_key, unobserved(1, 1));
            assert_eq!(spool_now(&world, spool.id).trust, SpoolTrust::Verified);
        }
        assert!(account(&world).is_empty(), "a repeated run found something");
    }
}

/// Two unseen completed prints of one file are charged twice under two keys;
/// after the file's tally is reset, a new unseen print reusing an ordinal is
/// charged again under the new generation — each debited once.
#[test]
fn ordinals_reused_after_a_reset_are_new_keys() {
    let world = spool_world();
    let spool = loaded_spool(&world, "Reused Ordinal", 1_000.0);
    world
        .printer
        .holds_file(printobserver_printer_api::FileFacts {
            file_name: "calibration.gcode".to_owned(),
            successes: 1,
            ..world.printer.file(FILE)
        });
    account(&world);
    tally(&world, 2, 0, true);
    account(&world);
    tally(&world, 1, 0, true);
    account(&world);

    let keys: Vec<ChargeKey> = charges_to(&world, spool.id)
        .into_iter()
        .map(|charge| charge.charge_key)
        .collect();
    assert_eq!(
        keys,
        vec![unobserved(0, 1), unobserved(0, 2), unobserved(1, 1)]
    );
    assert_eq!(every::<UnaccountedPrintPayload>(&world).len(), 3);
    assert_close(
        spool_now(&world, spool.id).remaining_g,
        1_000.0 - 3.0 * grams(&spool, TOTAL_MM),
    );
    assert_ledger_explains(&world, spool.id);
}

/// A run interrupted at its write — refused outright, or committed with its
/// answer lost — followed by another leaves each unseen print charged once,
/// its event written once and the tallies advanced: the charges, events and
/// tallies take effect together.
#[test]
fn an_interrupted_run_accounts_each_print_once() {
    for lost in [false, true] {
        let world = spool_world();
        let spool = loaded_spool(&world, "Interrupted", 1_000.0);
        tally(&world, 1, 0, true);
        account(&world);
        tally(&world, 3, 0, true);
        let error = StoreError::Database {
            detail: "the connection dropped".to_owned(),
        };
        if lost {
            world
                .store
                .loses_the_answer_to(StoreMethod::AccountTallies, error);
        } else {
            world.store.fails(StoreMethod::AccountTallies, error);
        }
        world.clock.advance(60);
        block_on(world.core.account_unobserved_prints()).expect_err("the write failed");
        if !lost {
            assert!(every::<UnaccountedPrintPayload>(&world).is_empty());
            assert!(charges_to(&world, spool.id).is_empty());
        }
        world.store.heals();
        account(&world);
        account(&world);

        assert_eq!(
            every::<UnaccountedPrintPayload>(&world).len(),
            2,
            "lost={lost}"
        );
        let keys: Vec<ChargeKey> = charges_to(&world, spool.id)
            .into_iter()
            .map(|charge| charge.charge_key)
            .collect();
        assert_eq!(
            keys,
            vec![unobserved(0, 2), unobserved(0, 3)],
            "lost={lost}"
        );
        assert_close(
            spool_now(&world, spool.id).remaining_g,
            1_000.0 - 2.0 * grams(&spool, TOTAL_MM),
        );
    }
}

/// A printer whose files cannot be listed is answered as the printer's own
/// error, and the run records nothing and advances no tally.
#[test]
fn a_printer_that_cannot_list_its_files_is_answered_and_nothing_is_recorded() {
    let world = spool_world();
    let spool = loaded_spool(&world, "Unlisted", 1_000.0);
    tally(&world, 1, 0, true);
    account(&world);
    tally(&world, 2, 0, true);
    let refusal = printobserver_printer_api::PrinterError::Unreachable {
        detail: "the printer is unreachable".to_owned(),
    };
    world
        .printer
        .fails(crate::fakes::PrinterMethod::PrintedFiles, refusal.clone());
    assert_eq!(
        block_on(world.core.account_unobserved_prints()),
        Err(printobserver_core::CoreError::Printer(refusal))
    );
    assert!(every::<UnaccountedPrintPayload>(&world).is_empty());
    world.printer.heals();
    account(&world);
    assert_eq!(every::<UnaccountedPrintPayload>(&world).len(), 1);
    assert_eq!(charges_to(&world, spool.id).len(), 1);
}

/// A print this system cancelled, counted by the printer's tally as failed,
/// is one it saw: the accounting flags nothing and marks nothing for it.
#[test]
fn a_print_this_system_cancelled_is_not_flagged_when_the_tally_counts_it_failed() {
    let world = spool_world();
    let spool = loaded_spool(&world, "Cancelled And Tallied", 1_000.0);
    tally(&world, 1, 0, true);
    account(&world);
    start(&world);
    running_at(&world, 200_000);
    world
        .printer
        .extrudes(FILE, 200_000, vec![crate::shelf::on_tool(0, 900.0)]);
    ends(&world, PrinterState::Cancelling);
    tally_failure(&world, world.clock.now());
    assert!(account(&world).is_empty());
    assert_eq!(spool_now(&world, spool.id).trust, SpoolTrust::Verified);
}
