//! The filament ledger survives its store being dropped and the file reopened.
//!
//! Every part of the ledger is written to a database on disk and read back in
//! full, the store is dropped, and a store opened afresh on the same file reads
//! every part back unchanged — and a replay submitted to the reopened store
//! still finds its key recorded, so a restart is never a second debit.

use std::sync::Arc;

use crate::block_on::block_on;
use crate::fixture::Store;
use crate::ledger_records::{
    Change, LedgerState, at, change, charge, ended, ledger_state, newest_timeline_events, pend,
    print_key, register, segment_key, tally, unaccounted, unobserved_key,
};
use printobserver_core::store::Settlement;
use printobserver_core::{FilePosition, PrintOutcome, SpoolId, UnverifiedReason};
use printobserver_store_sqlite::SqliteStore;
use printobserver_types::PrintId;
use tempfile::TempDir;

/// What the first store wrote, read back in full before it was dropped.
struct Written {
    /// The two prints the ledger names.
    prints: [PrintId; 2],
    /// The spool charged.
    spool: SpoolId,
    /// Everything the ledger held.
    state: LedgerState,
    /// The first print's file position.
    position: Option<FilePosition>,
}

/// Write one of everything the ledger holds.
fn write_the_ledger(port: &Arc<dyn Store>) -> Written {
    let first =
        block_on(port.open_print(None, Some("bracket.gcode".to_owned()))).expect("a print opens");
    let second =
        block_on(port.open_print(None, Some("arm.gcode".to_owned()))).expect("a print opens");
    let spool = register(port, "charged", Some(0), 800.0, at(0));
    let retired = register(port, "empty", None, 3.0, at(1));
    block_on(port.apply_spool_changes(
        vec![
            change(retired.id, Change::Retire, at(2)),
            change(
                spool.id,
                Change::MarkUnverified {
                    reason: UnverifiedReason::UnchargedPrint {
                        print_id: second.id,
                    },
                },
                at(2),
            ),
        ],
        vec![charge(
            print_key(first.id, 0, 0),
            Some(first.id),
            Some(spool.id),
            42.5,
            at(3),
        )],
        vec![pend(segment_key(first.id, 0, 1), at(4))],
    ))
    .expect("the ledger writes");
    block_on(port.record_file_position(
        first.id,
        FilePosition {
            bytes: 7_000,
            size_bytes: Some(10_000),
            observed_at: at(5),
        },
    ))
    .expect("a position is recorded");
    block_on(port.settle_print(
        second.id,
        PrintOutcome::Failed,
        ended(second.id, PrintOutcome::Failed, at(6)),
        vec![charge(
            print_key(second.id, 0, 0),
            Some(second.id),
            Some(spool.id),
            7.5,
            at(6),
        )],
        Vec::new(),
        Vec::new(),
    ))
    .expect("the print settles");
    block_on(port.account_tallies(
        vec![
            tally("bracket.gcode", 3, 2, at(7)),
            tally("arm.gcode", 0, 1, at(7)),
        ],
        vec![charge(
            unobserved_key("bracket.gcode", 3, 2),
            None,
            Some(spool.id),
            10.0,
            at(7),
        )],
        Vec::new(),
        vec![unaccounted("bracket.gcode", at(7))],
    ))
    .expect("the tallies account");

    let prints = [first.id, second.id];
    Written {
        prints,
        spool: spool.id,
        state: ledger_state(port, &prints),
        position: block_on(port.file_position(first.id)).expect("the position reads"),
    }
}

/// Submit every key the first store recorded again, which writes nothing.
fn replay_against(port: &Arc<dyn Store>, written: &Written) {
    let [first_print, second_print] = written.prints;
    let events = newest_timeline_events(port).len();
    let replay = block_on(port.apply_spool_changes(
        Vec::new(),
        vec![charge(
            print_key(first_print, 0, 0),
            Some(first_print),
            Some(written.spool),
            42.5,
            at(9),
        )],
        vec![pend(segment_key(first_print, 0, 1), at(9))],
    ))
    .expect("the replay answers");
    assert_eq!(replay.events, Vec::new(), "a replay after a restart wrote");
    assert_eq!(
        block_on(port.settle_print(
            second_print,
            PrintOutcome::Completed,
            ended(second_print, PrintOutcome::Completed, at(9)),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )),
        Ok(Settlement::AlreadySettled {
            at: at(6),
            outcome: PrintOutcome::Failed,
        })
    );
    assert_eq!(
        block_on(port.account_tallies(
            Vec::new(),
            vec![charge(
                unobserved_key("bracket.gcode", 3, 2),
                None,
                Some(written.spool),
                10.0,
                at(9),
            )],
            Vec::new(),
            Vec::new(),
        )),
        Ok(Vec::new()),
        "an unobserved charge replayed after a restart wrote"
    );
    assert_eq!(newest_timeline_events(port).len(), events);
}

/// Every part of the ledger reads back unchanged from a reopened file, and a
/// replay against the reopened store writes nothing.
#[test]
fn the_ledger_reads_back_unchanged_from_a_reopened_file_and_a_replay_writes_nothing() {
    let dir = TempDir::new().expect("a temporary state directory");
    let first: Arc<dyn Store> = Arc::new(SqliteStore::open(dir.path()).expect("the store opens"));
    let written = write_the_ledger(&first);
    drop(first);

    let port: Arc<dyn Store> = Arc::new(SqliteStore::open(dir.path()).expect("the store reopens"));
    let first_print = written.prints[0];
    assert_eq!(
        ledger_state(&port, &written.prints),
        written.state,
        "the ledger read back differently from the reopened file"
    );
    assert_eq!(
        written
            .state
            .tallies
            .iter()
            .map(|held| (held.file_name.as_str(), held.generation))
            .collect::<Vec<_>>(),
        vec![("arm.gcode", 0), ("bracket.gcode", 3)],
        "the tallies written were not the ones this journey reads back"
    );
    assert_eq!(
        written.state.pending.len(),
        1,
        "no pending charge was written"
    );
    assert_eq!(
        written.state.spools.len(),
        2,
        "both spools were not written"
    );
    assert!(
        written
            .state
            .spools
            .iter()
            .any(printobserver_core::SpoolRecord::is_retired),
        "the retired spool was not written"
    );
    assert_eq!(
        written.state.settlements[1],
        Some((at(6), PrintOutcome::Failed))
    );
    assert_eq!(
        block_on(port.file_position(first_print)).expect("the position reads"),
        written.position
    );
    assert_eq!(
        block_on(port.loaded_spools())
            .expect("the loaded spools read")
            .iter()
            .map(|spool| spool.id)
            .collect::<Vec<_>>(),
        vec![written.spool]
    );

    replay_against(&port, &written);
    assert_eq!(
        ledger_state(&port, &written.prints),
        written.state,
        "a replay against the reopened store changed the ledger"
    );
}
