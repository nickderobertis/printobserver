//! The filament ledger and the printer timeline, against both stores.
//!
//! Every journey here runs against the durable store and the in-memory one, so
//! the rules `FilamentStore` and `TimelineStore` state — the replay rules, what
//! refuses a change, which change writes an event, and the all-or-nothing write
//! — are held of both. `port_coverage.rs` reads this file beside the rest of the
//! suite, so a ledger method it never calls is refused.

use std::sync::Arc;

use crate::block_on::block_on;
use crate::fixture::{Fixture, Store, draft};
use crate::ledger_records::{
    Change, at, change, charge, ended, event_of, kinds, ledger_state, new_spool,
    newest_timeline_events, pend, print_key, register, segment_key, tally, unaccounted,
    unobserved_key, whole_timeline,
};
use printobserver_core::kinds::{
    PrintStartedPayload, SpoolUnverifiedPayload, operator_source, system_source,
};
use printobserver_core::store::{
    MAX_HISTORY_LIMIT, Settlement, SpoolMutation, StoreError, TimelineQuery,
};
use printobserver_core::{
    Actor, FilePosition, PrintOutcome, PrintRecord, SpoolId, SpoolTrust, UnverifiedMark,
    UnverifiedReason, UnverifiedReasonKind,
};
use printobserver_types::{EventKind, EventPayload, EventRecord, PrintId};

/// A print, opened.
fn open(port: &Arc<dyn Store>, file_name: &str) -> PrintRecord {
    block_on(port.open_print(None, Some(file_name.to_owned()))).expect("a print opens")
}

/// A refusal naming one constraint.
fn refused(constraint: &str) -> StoreError {
    StoreError::ConstraintRefused {
        constraint: constraint.to_owned(),
    }
}

/// One spool's remaining figure as the store holds it now.
fn remaining(port: &Arc<dyn Store>, spool_id: SpoolId) -> f64 {
    block_on(port.spool(spool_id))
        .expect("the spool reads")
        .expect("the spool exists")
        .remaining_g
}

/// Assert a figure in grams is the one expected.
///
/// Every figure these journeys write is a sum of halves, which a float holds
/// exactly; the tolerance is only there so the comparison says so.
#[track_caller]
fn assert_grams(actual: f64, expected: f64, context: &str) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "{context}: {actual} g where {expected} g was expected"
    );
}

/// The reason a print nothing could be charged for marks a spool with.
const fn uncharged(print_id: PrintId) -> UnverifiedReason {
    UnverifiedReason::UnchargedPrint { print_id }
}

/// The reason a filament change nobody confirmed marks a spool with.
const fn unconfirmed(print_id: PrintId) -> UnverifiedReason {
    UnverifiedReason::FilamentChangeUnconfirmed { print_id }
}

/// How many `spool_unverified` events name one spool.
fn unverified_events(port: &Arc<dyn Store>, spool_id: SpoolId) -> usize {
    newest_timeline_events(port)
        .iter()
        .filter_map(EventRecord::payload_as::<SpoolUnverifiedPayload>)
        .filter(|payload| payload.as_ref().is_ok_and(|read| read.spool_id == spool_id))
        .count()
}

/// A registration answers the spool and its event from one write.
#[test]
fn a_registration_answers_the_spool_and_its_event_from_one_write() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let (spool, event) = block_on(port.register_spool(
            new_spool("shelf PLA", Some(0), 800.0, at(0)),
            operator_source(),
            Actor::Operator,
            "a new spool on the shelf".to_owned(),
        ))
        .expect("the spool registers");

        assert_eq!(spool.updated_at, spool.registered_at, "{name}");
        assert!(!spool.is_retired(), "{name}");
        assert_eq!((spool.remaining_g, spool.loaded_tool()), (800.0, Some(0)));
        assert_eq!(event.print_id, None, "{name}: a registration names a print");
        assert_eq!(event.received_at, at(0), "{name}");
        assert_eq!(event.kind().as_str(), "spool_registered", "{name}");
        let payload = event
            .payload_as::<printobserver_core::SpoolRegisteredPayload>()
            .expect("a registration payload")
            .expect("which reads");
        assert_eq!(
            payload.spool, spool,
            "{name}: the payload is not the record"
        );
        assert_eq!(block_on(port.spool(spool.id)), Ok(Some(spool.clone())));
        assert_eq!(newest_timeline_events(&port), vec![event], "{name}");

        let later = register(&port, "second", None, 500.0, at(5));
        assert_eq!(
            block_on(port.spools(false)),
            Ok(vec![later, spool]),
            "{name}: the spools are not most recently registered first"
        );
        let before = ledger_state(&port, &[]);
        assert_eq!(
            block_on(port.register_spool(
                new_spool("third", Some(0), 1_000.0, at(9)),
                operator_source(),
                Actor::Operator,
                "onto an occupied tool".to_owned(),
            )),
            Err(refused("spools.loaded_on_tool")),
            "{name}"
        );
        assert_eq!(ledger_state(&port, &[]), before, "{name}: a refusal wrote");
    }
}

/// A swap — an unload and then a load in one call — leaves one spool on the tool.
#[test]
fn a_swap_leaves_exactly_one_spool_on_the_tool() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let outgoing = register(&port, "outgoing", Some(0), 300.0, at(0));
        let incoming = register(&port, "incoming", None, 900.0, at(1));
        let other = register(&port, "other tool", Some(1), 700.0, at(2));

        let write = block_on(port.apply_spool_changes(
            vec![
                change(outgoing.id, Change::Unload, at(10)),
                change(incoming.id, Change::Load { tool: 0 }, at(10)),
            ],
            Vec::new(),
            Vec::new(),
        ))
        .expect("the swap applies");

        assert_eq!(
            write
                .spools
                .iter()
                .map(|spool| (spool.id, spool.loaded_tool()))
                .collect::<Vec<_>>(),
            vec![(outgoing.id, None), (incoming.id, Some(0))],
            "{name}: the write does not answer both spools as it left them"
        );
        assert_eq!(kinds(&write.events), ["spool_unloaded", "spool_loaded"]);
        let loaded = block_on(port.loaded_spools()).expect("the loaded spools read");
        assert_eq!(
            loaded
                .iter()
                .map(|spool| (spool.loaded_tool(), spool.id))
                .collect::<Vec<_>>(),
            vec![(Some(0), incoming.id), (Some(1), other.id)],
            "{name}: the tools do not carry exactly one spool each"
        );
        let swapped = block_on(port.spool(incoming.id))
            .expect("the spool reads")
            .expect("it exists");
        assert_eq!(swapped.updated_at, at(10), "{name}");
    }
}

/// Every refused change refuses the whole call, naming its constraint.
#[test]
fn a_refused_change_names_its_constraint_and_leaves_no_partial_write() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let print = open(&port, "bracket.gcode");
        let held = register(&port, "on tool 0", Some(0), 500.0, at(0));
        let spare = register(&port, "spare", None, 500.0, at(1));
        let retired = register(&port, "empty", None, 5.0, at(2));
        block_on(port.apply_spool_changes(
            vec![change(retired.id, Change::Retire, at(3))],
            Vec::new(),
            Vec::new(),
        ))
        .expect("the spool retires");

        // Each refused call carries a charge and a change that would apply
        // ahead of the one refused, so a partial write would show.
        let attempts: Vec<(Change, SpoolId, StoreError)> = vec![
            (
                Change::Load { tool: 0 },
                spare.id,
                refused("spools.loaded_on_tool"),
            ),
            (
                Change::Load { tool: 3 },
                retired.id,
                refused("spools.retired_at"),
            ),
            (Change::Unload, retired.id, refused("spools.retired_at")),
            (
                Change::SetRemaining { remaining_g: 900.0 },
                retired.id,
                refused("spools.retired_at"),
            ),
            (
                Change::ClearMarks {
                    kinds: vec![UnverifiedReasonKind::UnchargedPrint],
                },
                retired.id,
                refused("spools.retired_at"),
            ),
            (Change::Retire, retired.id, refused("spools.retired_at")),
            (
                Change::Unload,
                SpoolId::new(),
                StoreError::NotFound {
                    what: String::new(),
                },
            ),
        ];
        let before = ledger_state(&port, &[print.id]);
        for (index, (refused_change, spool_id, expected)) in attempts.into_iter().enumerate() {
            let answer = block_on(port.apply_spool_changes(
                vec![
                    change(held.id, Change::SetRemaining { remaining_g: 1.0 }, at(20)),
                    change(spool_id, refused_change, at(20)),
                ],
                vec![charge(
                    print_key(print.id, 0, u32::try_from(index).expect("a small index")),
                    Some(print.id),
                    Some(held.id),
                    10.0,
                    at(20),
                )],
                Vec::new(),
            ));
            match (&answer, &expected) {
                (Err(StoreError::NotFound { what }), StoreError::NotFound { .. }) => {
                    assert!(what.contains(&spool_id.to_string()), "{name}: {what}");
                }
                _ => assert_eq!(answer, Err(expected), "{name}"),
            }
            assert_eq!(
                ledger_state(&port, &[print.id]),
                before,
                "{name}: a refused call left part of its write behind"
            );
        }
    }
}

/// A charge is recorded below zero, and against a retired spool.
#[test]
fn a_charge_below_zero_is_recorded_and_one_against_a_retired_spool_applied() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let print = open(&port, "bracket.gcode");
        let spool = register(&port, "nearly empty", Some(0), 10.0, at(0));

        let write = block_on(port.apply_spool_changes(
            Vec::new(),
            vec![charge(
                print_key(print.id, 0, 0),
                Some(print.id),
                Some(spool.id),
                25.0,
                at(5),
            )],
            Vec::new(),
        ))
        .expect("the charge applies");
        assert_grams(write.spools[0].remaining_g, -15.0, "{name}");
        assert_eq!(write.spools[0].updated_at, at(5), "{name}");
        assert_eq!(kinds(&write.events), ["filament_consumed"]);
        assert_eq!(write.charges[0].event_id, write.events[0].id, "{name}");

        block_on(port.apply_spool_changes(
            vec![
                change(spool.id, Change::Unload, at(6)),
                change(spool.id, Change::Retire, at(6)),
            ],
            Vec::new(),
            Vec::new(),
        ))
        .expect("the spool retires");
        block_on(port.apply_spool_changes(
            Vec::new(),
            vec![charge(
                print_key(print.id, 0, 1),
                Some(print.id),
                Some(spool.id),
                5.0,
                at(7),
            )],
            Vec::new(),
        ))
        .expect("a charge against a retired spool applies");
        assert_grams(remaining(&port, spool.id), -20.0, "{name}");
        assert_eq!(
            block_on(port.spools(false)),
            Ok(Vec::new()),
            "{name}: a retired spool is listed without being asked for"
        );
        assert_eq!(
            block_on(port.charges(print.id))
                .expect("the charges read")
                .iter()
                .map(|record| record.identity.key().clone())
                .collect::<Vec<_>>(),
            vec![print_key(print.id, 0, 0), print_key(print.id, 0, 1)],
            "{name}"
        );
    }
}

/// Marks are kept one per reason, cleared by kind, and wiped by a correction.
#[test]
fn marks_are_kept_per_reason_cleared_by_kind_and_wiped_by_a_correction() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let print = open(&port, "bracket.gcode");
        let spool = register(&port, "doubted", Some(0), 500.0, at(0));
        let mark = |reason: UnverifiedReason, minute: u32| {
            block_on(port.apply_spool_changes(
                vec![change(
                    spool.id,
                    Change::MarkUnverified { reason },
                    at(minute),
                )],
                Vec::new(),
                Vec::new(),
            ))
            .expect("the mark applies")
        };

        mark(uncharged(print.id), 1);
        mark(unconfirmed(print.id), 2);
        let repeated = mark(uncharged(print.id), 3);
        assert_eq!(repeated.events, Vec::new(), "{name}: a repeated mark wrote");
        let both = SpoolTrust::Unverified {
            marks: vec![
                UnverifiedMark {
                    reason: uncharged(print.id),
                    since: at(1),
                },
                UnverifiedMark {
                    reason: unconfirmed(print.id),
                    since: at(2),
                },
            ],
        };
        assert_eq!(repeated.spools[0].trust, both, "{name}");
        assert_eq!(unverified_events(&port, spool.id), 2, "{name}");

        let cleared = block_on(port.apply_spool_changes(
            vec![change(
                spool.id,
                Change::ClearMarks {
                    kinds: vec![UnverifiedReasonKind::FilamentChangeUnconfirmed],
                },
                at(4),
            )],
            Vec::new(),
            Vec::new(),
        ))
        .expect("the marks clear");
        assert_eq!(
            cleared.events,
            Vec::new(),
            "{name}: clearing wrote an event"
        );
        assert_eq!(
            cleared.spools[0].trust,
            SpoolTrust::Unverified {
                marks: vec![UnverifiedMark {
                    reason: uncharged(print.id),
                    since: at(1),
                }],
            },
            "{name}: clearing one kind did not leave the other"
        );

        let corrected = block_on(port.apply_spool_changes(
            vec![change(
                spool.id,
                Change::SetRemaining { remaining_g: 420.0 },
                at(5),
            )],
            Vec::new(),
            Vec::new(),
        ))
        .expect("the correction applies");
        assert_eq!(corrected.spools[0].trust, SpoolTrust::Verified, "{name}");
        assert_grams(corrected.spools[0].remaining_g, 420.0, "{name}");
        assert_eq!(kinds(&corrected.events), ["spool_adjusted"]);
    }
}

/// A retired spool still takes a settlement's mark, and refuses a clearing.
#[test]
fn a_retired_spool_takes_a_mark_and_refuses_a_clearing() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let print = open(&port, "bracket.gcode");
        let spool = register(&port, "retiring", None, 500.0, at(0));
        block_on(port.apply_spool_changes(
            vec![change(spool.id, Change::Retire, at(6))],
            Vec::new(),
            Vec::new(),
        ))
        .expect("the spool retires");
        let marked = block_on(port.apply_spool_changes(
            vec![change(
                spool.id,
                Change::MarkUnverified {
                    reason: unconfirmed(print.id),
                },
                at(7),
            )],
            Vec::new(),
            Vec::new(),
        ))
        .expect("the mark applies");
        assert_eq!(kinds(&marked.events), ["spool_unverified"], "{name}");
        assert_eq!(
            block_on(port.apply_spool_changes(
                vec![change(
                    spool.id,
                    Change::ClearMarks {
                        kinds: vec![UnverifiedReasonKind::FilamentChangeUnconfirmed],
                    },
                    at(8),
                )],
                Vec::new(),
                Vec::new(),
            )),
            Err(refused("spools.retired_at")),
            "{name}"
        );
    }
}

/// The latest file position is kept by when it was read.
#[test]
fn the_latest_file_position_is_kept_by_when_it_was_read() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let print = open(&port, "bracket.gcode");
        let position = |bytes: i64, minute: u32| FilePosition {
            bytes,
            size_bytes: Some(10_000),
            observed_at: at(minute),
        };
        assert_eq!(block_on(port.file_position(print.id)), Ok(None), "{name}");
        block_on(port.record_file_position(print.id, position(500, 10))).expect("recorded");
        block_on(port.record_file_position(print.id, position(200, 5))).expect("recorded");
        assert_eq!(
            block_on(port.file_position(print.id)),
            Ok(Some(position(500, 10))),
            "{name}: an older position replaced a newer one"
        );
        block_on(port.record_file_position(print.id, position(900, 20))).expect("recorded");
        assert_eq!(
            block_on(port.file_position(print.id)),
            Ok(Some(position(900, 20))),
            "{name}"
        );
        block_on(port.record_file_position(print.id, position(950, 20))).expect("recorded");
        assert_eq!(
            block_on(port.file_position(print.id)),
            Ok(Some(position(950, 20))),
            "{name}: a position read at the same instant did not replace the one held"
        );
        assert!(
            matches!(
                block_on(port.record_file_position(PrintId::new(), position(1, 1))),
                Err(StoreError::NotFound { .. })
            ),
            "{name}: a position for no print was not refused as not found"
        );
    }
}

/// A charge already recorded is a replay wherever it is submitted again.
#[test]
fn a_replayed_charge_debits_nothing_and_writes_no_event_through_every_write() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let print = open(&port, "bracket.gcode");
        let other = open(&port, "bracket.gcode");
        let spool = register(&port, "charged", Some(0), 500.0, at(0));
        let debit = || {
            charge(
                print_key(print.id, 0, 0),
                Some(print.id),
                Some(spool.id),
                12.5,
                at(5),
            )
        };
        let first = block_on(port.apply_spool_changes(Vec::new(), vec![debit()], Vec::new()))
            .expect("the charge applies");
        let recorded = first.charges[0].clone();
        let events = newest_timeline_events(&port);
        let charges = block_on(port.charges(print.id)).expect("the charges read");
        assert_grams(remaining(&port, spool.id), 487.5, "{name}");

        let again = block_on(port.apply_spool_changes(Vec::new(), vec![debit()], Vec::new()))
            .expect("the replay answers");
        assert_eq!(
            again.charges,
            vec![recorded],
            "{name}: not the existing record"
        );
        assert_eq!(
            again.events,
            Vec::new(),
            "{name}: the replay wrote an event"
        );

        let settled = block_on(port.settle_print(
            other.id,
            PrintOutcome::Completed,
            ended(other.id, PrintOutcome::Completed, at(30)),
            vec![debit()],
            Vec::new(),
            Vec::new(),
        ))
        .expect("the settlement applies");
        let Settlement::Settled {
            events: written, ..
        } = settled
        else {
            panic!("{name}: a first settlement answered {settled:?}");
        };
        assert_eq!(kinds(&written), ["print_ended"], "{name}: the replay wrote");

        let accounted = block_on(port.account_tallies(
            vec![tally("bracket.gcode", 0, 2, at(40))],
            vec![debit()],
            Vec::new(),
            Vec::new(),
        ))
        .expect("the tallies account");
        assert_eq!(accounted, Vec::new(), "{name}: the replay wrote an event");

        assert_grams(
            remaining(&port, spool.id),
            487.5,
            "{name}: a replay debited",
        );
        assert_eq!(block_on(port.charges(print.id)), Ok(charges), "{name}");
        let after: Vec<EventRecord> = newest_timeline_events(&port)
            .into_iter()
            .filter(|event| event.kind().as_str() == "filament_consumed")
            .collect();
        assert_eq!(
            after,
            events
                .into_iter()
                .filter(|event| event.kind().as_str() == "filament_consumed")
                .collect::<Vec<_>>(),
            "{name}: a replay appended a filament_consumed event"
        );
    }
}

/// Two prints of one file are two charges, however their keys differ.
#[test]
fn two_prints_of_one_file_are_two_charges() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let first = open(&port, "bracket.gcode");
        let second = open(&port, "bracket.gcode");
        let spool = register(&port, "charged", Some(0), 500.0, at(0));
        let debit =
            |key, print_id: Option<PrintId>| charge(key, print_id, Some(spool.id), 10.0, at(5));
        let write = block_on(port.apply_spool_changes(
            Vec::new(),
            vec![
                debit(print_key(first.id, 0, 0), Some(first.id)),
                debit(print_key(second.id, 0, 0), Some(second.id)),
                debit(unobserved_key("bracket.gcode", 0, 1), None),
                debit(unobserved_key("bracket.gcode", 0, 2), None),
                debit(unobserved_key("bracket.gcode", 1, 2), None),
            ],
            Vec::new(),
        ))
        .expect("the charges apply");
        assert_eq!(
            write.events.len(),
            5,
            "{name}: a distinct print was a replay"
        );
        assert_grams(remaining(&port, spool.id), 450.0, "{name}");
        assert_eq!(
            block_on(port.charges(first.id)).map(|found| found.len()),
            Ok(1)
        );
        assert_eq!(
            block_on(port.charges(second.id)).map(|found| found.len()),
            Ok(1)
        );
    }
}

/// A tally keeps its generation, and one submitted below it is refused.
#[test]
fn a_tally_keeps_its_generation_and_one_below_it_is_refused_writing_nothing() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let spool = register(&port, "charged", Some(0), 500.0, at(0));
        let written = block_on(port.account_tallies(
            vec![
                tally("bracket.gcode", 2, 1, at(10)),
                tally("arm.gcode", 0, 4, at(10)),
            ],
            vec![charge(
                unobserved_key("bracket.gcode", 2, 1),
                None,
                Some(spool.id),
                30.0,
                at(10),
            )],
            vec![change(
                spool.id,
                Change::MarkUnverified {
                    reason: UnverifiedReason::TallyReset {
                        file_name: "bracket.gcode".to_owned(),
                        origin: "local".to_owned(),
                    },
                },
                at(10),
            )],
            vec![unaccounted("bracket.gcode", at(10))],
        ))
        .expect("the tallies account");
        assert_eq!(
            kinds(&written),
            ["filament_consumed", "spool_unverified", "unaccounted_print"],
            "{name}"
        );
        assert_eq!(
            block_on(port.file_tallies()),
            Ok(vec![
                tally("arm.gcode", 0, 4, at(10)),
                tally("bracket.gcode", 2, 1, at(10)),
            ]),
            "{name}: the tallies did not read back with their generations"
        );
        assert_grams(remaining(&port, spool.id), 470.0, "{name}");

        let before = ledger_state(&port, &[]);
        assert_eq!(
            block_on(port.account_tallies(
                vec![
                    tally("arm.gcode", 0, 5, at(20)),
                    tally("bracket.gcode", 1, 9, at(20)),
                ],
                vec![charge(
                    unobserved_key("arm.gcode", 0, 5),
                    None,
                    Some(spool.id),
                    30.0,
                    at(20),
                )],
                Vec::new(),
                vec![unaccounted("arm.gcode", at(20))],
            )),
            Err(refused("file_tallies.generation")),
            "{name}"
        );
        assert_eq!(ledger_state(&port, &[]), before, "{name}: a refusal wrote");

        block_on(port.account_tallies(
            vec![tally("bracket.gcode", 2, 3, at(30))],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ))
        .expect("a tally at the held generation advances");
        assert_eq!(
            block_on(port.file_tallies()).map(|found| found[1].successes),
            Ok(3),
            "{name}"
        );
    }
}

/// Settle a print as cancelled, with one charge, one pend and these marks.
fn settle_cancelled(
    port: &Arc<dyn Store>,
    print_id: PrintId,
    spool_id: SpoolId,
    marks: Vec<SpoolMutation>,
) -> Result<Settlement, StoreError> {
    block_on(port.settle_print(
        print_id,
        PrintOutcome::Cancelled,
        ended(print_id, PrintOutcome::Cancelled, at(30)),
        vec![charge(
            print_key(print_id, 0, 0),
            Some(print_id),
            Some(spool_id),
            20.0,
            at(30),
        )],
        vec![pend(segment_key(print_id, 0, 1), at(30))],
        marks,
    ))
}

/// Settling a print nothing opened is refused as not found.
#[test]
fn settling_no_print_is_refused_as_not_found() {
    for store in Fixture::both() {
        let port = store.port();
        assert!(
            matches!(
                block_on(port.settle_print(
                    PrintId::new(),
                    PrintOutcome::Completed,
                    draft(None, "obico_failure_alert", at(1)),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                )),
                Err(StoreError::NotFound { .. })
            ),
            "{}: settling no print was not refused as not found",
            store.name()
        );
    }
}

/// A settlement writes every part of itself once, and the second writes nothing.
#[test]
fn a_print_is_settled_in_one_write_and_only_once() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let print = open(&port, "bracket.gcode");
        let spool = register(&port, "charged", Some(0), 500.0, at(0));
        let settle = |marks| settle_cancelled(&port, print.id, spool.id, marks);

        // A settlement refused part way leaves nothing, the settlement included.
        let before = ledger_state(&port, &[print.id]);
        assert!(
            matches!(
                settle(vec![change(
                    SpoolId::new(),
                    Change::MarkUnverified {
                        reason: unconfirmed(print.id),
                    },
                    at(30),
                )]),
                Err(StoreError::NotFound { .. })
            ),
            "{name}: a mark on no spool was not refused"
        );
        assert_eq!(ledger_state(&port, &[print.id]), before, "{name}");

        let answer = settle(vec![change(
            spool.id,
            Change::MarkUnverified {
                reason: unconfirmed(print.id),
            },
            at(30),
        )])
        .expect("the print settles");
        let Settlement::Settled {
            at: settled_at,
            events,
        } = answer
        else {
            panic!("{name}: a first settlement answered {answer:?}");
        };
        assert_eq!(settled_at, at(30), "{name}");
        assert_eq!(
            kinds(&events),
            [
                "filament_consumed",
                "charge_pended",
                "spool_unverified",
                "print_ended"
            ],
            "{name}"
        );
        assert_eq!(
            block_on(port.print_settlement(print.id)),
            Ok(Some((at(30), PrintOutcome::Cancelled))),
            "{name}"
        );
        assert_grams(remaining(&port, spool.id), 480.0, "{name}");
        assert_eq!(
            block_on(port.pending_charges(Some(0))).map(|found| found.len()),
            Ok(1),
            "{name}"
        );
        assert_eq!(
            block_on(port.print(print.id)).map(|found| found.and_then(|p| p.ended_at)),
            Ok(None),
            "{name}: settling ended the print record"
        );

        let after = ledger_state(&port, &[print.id]);
        assert_eq!(
            settle(Vec::new()),
            Ok(Settlement::AlreadySettled {
                at: at(30),
                outcome: PrintOutcome::Cancelled,
            }),
            "{name}"
        );
        assert_eq!(ledger_state(&port, &[print.id]), after, "{name}");
    }
}

/// A pend is listed, held once, and debited once by the charge that takes it.
#[test]
fn a_pend_is_listed_held_once_and_debited_once_by_the_charge_that_takes_it() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let print = open(&port, "bracket.gcode");
        let later = open(&port, "arm.gcode");
        let spool = register(&port, "next", None, 500.0, at(0));
        let held = pend(segment_key(print.id, 0, 1), at(20));

        let write = block_on(port.apply_spool_changes(
            Vec::new(),
            Vec::new(),
            vec![held.clone(), pend(segment_key(print.id, 1, 0), at(25))],
        ))
        .expect("the pends hold");
        assert_eq!(kinds(&write.events), ["charge_pended", "charge_pended"]);
        assert_eq!(write.pending.len(), 2, "{name}");
        block_on(port.settle_print(
            later.id,
            PrintOutcome::Completed,
            ended(later.id, PrintOutcome::Completed, at(40)),
            Vec::new(),
            vec![pend(segment_key(later.id, 0, 0), at(10))],
            Vec::new(),
        ))
        .expect("the settlement pends");
        let listed = |tool| {
            block_on(port.pending_charges(tool))
                .expect("the pending charges read")
                .into_iter()
                .map(|pending| pending.charge_key.charge_key())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            listed(Some(0)),
            vec![print_key(later.id, 0, 0), print_key(print.id, 0, 1)],
            "{name}: tool 0's pending charges are not oldest first"
        );
        assert_eq!(listed(None).len(), 3, "{name}");

        let replayed =
            block_on(port.apply_spool_changes(Vec::new(), Vec::new(), vec![held.clone()]))
                .expect("the replay answers");
        assert_eq!(
            (replayed.events, replayed.pending),
            (Vec::new(), Vec::new())
        );

        let taking = charge(
            print_key(print.id, 0, 1),
            Some(print.id),
            Some(spool.id),
            5.0,
            at(50),
        );
        let taken = block_on(port.apply_spool_changes(
            vec![change(spool.id, Change::Load { tool: 0 }, at(50))],
            vec![taking.clone()],
            Vec::new(),
        ))
        .expect("the load takes the pending charge");
        assert_eq!(kinds(&taken.events), ["filament_consumed", "spool_loaded"]);
        assert_eq!(listed(Some(0)), vec![print_key(later.id, 0, 0)], "{name}");
        assert_grams(remaining(&port, spool.id), 495.0, "{name}");

        let before = ledger_state(&port, &[print.id, later.id]);
        let again = block_on(port.apply_spool_changes(Vec::new(), vec![taking], vec![held]))
            .expect("the replays answer");
        assert_eq!(
            again.events,
            Vec::new(),
            "{name}: a replay after the take wrote"
        );
        assert_eq!(ledger_state(&port, &[print.id, later.id]), before, "{name}");
    }
}

/// The events the timeline journey writes, oldest first, and the one spool
/// named in some of them.
struct Timeline {
    /// Every event written, oldest first.
    written: Vec<EventRecord>,
    /// The spool registered on tool 0.
    spool: SpoolId,
}

/// Write events of timeline kinds and others, across two prints and none.
fn timeline_written(port: &Arc<dyn Store>) -> Timeline {
    let first = open(port, "bracket.gcode");
    let second = open(port, "arm.gcode");
    let started = |print_id: PrintId, minute| {
        block_on(port.append_event(event_of(
            Some(print_id),
            at(minute),
            &PrintStartedPayload {
                file_name: Some("bracket.gcode".to_owned()),
                started_by: None,
                loaded: Vec::new(),
                preflight: None,
            },
        )))
        .expect("the start is appended")
    };
    let alert = |print_id: PrintId, minute| {
        block_on(port.append_event(draft(Some(print_id), "obico_failure_alert", at(minute))))
            .expect("the alert is appended")
    };
    let mut written = Vec::new();
    written.push(started(first.id, 1));
    alert(first.id, 2);
    let (spool, registered) = block_on(port.register_spool(
        new_spool("tracked", Some(0), 500.0, at(3)),
        operator_source(),
        Actor::Operator,
        "a new spool".to_owned(),
    ))
    .expect("the spool registers");
    written.push(registered);
    written.push(started(second.id, 4));
    alert(second.id, 5);
    let charged = block_on(port.apply_spool_changes(
        Vec::new(),
        vec![charge(
            print_key(second.id, 0, 0),
            Some(second.id),
            Some(spool.id),
            1.0,
            at(6),
        )],
        Vec::new(),
    ))
    .expect("the charge applies");
    written.extend(charged.events);
    written.push(register_unrelated(port));
    Timeline {
        written,
        spool: spool.id,
    }
}

/// A second spool, registered last, which no other event names.
fn register_unrelated(port: &Arc<dyn Store>) -> EventRecord {
    block_on(port.register_spool(
        new_spool("unrelated", None, 500.0, at(7)),
        system_source(),
        Actor::System,
        "a second spool".to_owned(),
    ))
    .expect("the spool registers")
    .1
}

/// The identifiers of some events.
fn ids(events: &[EventRecord]) -> Vec<printobserver_types::EventId> {
    events.iter().map(|event| event.id).collect()
}

/// The timeline answers its kinds alone, newest first, under every filter.
#[test]
fn the_timeline_answers_its_kinds_newest_first_under_every_filter() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let Timeline { written, spool } = timeline_written(&port);
        let newest_first: Vec<EventRecord> = written.iter().rev().cloned().collect();
        let read = |query: TimelineQuery| block_on(port.timeline(query));

        assert_eq!(
            read(whole_timeline(None)).map(|found| ids(&found)),
            Ok(ids(&newest_first)),
            "{name}: the timeline is not its kinds alone, across prints and none, newest first"
        );
        let started = EventKind::new(PrintStartedPayload::KIND).expect("a kind name");
        assert_eq!(
            read(TimelineQuery {
                kinds: vec![started.clone()],
                ..whole_timeline(None)
            })
            .map(|found| ids(&found)),
            Ok(vec![written[2].id, written[0].id]),
            "{name}: the kinds filter"
        );
        assert_eq!(
            read(TimelineQuery {
                since: Some(at(3)),
                until: Some(at(6)),
                ..whole_timeline(None)
            })
            .map(|found| ids(&found)),
            Ok(vec![written[3].id, written[2].id, written[1].id]),
            "{name}: since and until are not inclusive bounds"
        );
        assert_eq!(
            read(TimelineQuery {
                spool_id: Some(spool),
                ..whole_timeline(None)
            })
            .map(|found| ids(&found)),
            Ok(vec![written[3].id, written[1].id]),
            "{name}: the spool filter"
        );
        assert_eq!(
            read(TimelineQuery {
                before: Some(written[2].id),
                ..whole_timeline(Some(1))
            })
            .map(|found| ids(&found)),
            Ok(vec![written[1].id]),
            "{name}: the cursor and the limit"
        );
        assert_eq!(
            read(TimelineQuery {
                kinds: vec![EventKind::new("obico_failure_alert").expect("a kind name")],
                ..whole_timeline(None)
            }),
            Err(refused("timeline.kinds")),
            "{name}: a kind outside the timeline"
        );
        assert_eq!(
            read(whole_timeline(Some(MAX_HISTORY_LIMIT + 1))),
            Err(StoreError::LimitRefused {
                limit: MAX_HISTORY_LIMIT,
                asked_for: MAX_HISTORY_LIMIT + 1,
            }),
            "{name}"
        );
        assert!(
            matches!(
                read(TimelineQuery {
                    before: Some(printobserver_types::EventId::new()),
                    ..whole_timeline(None)
                }),
                Err(StoreError::NotFound { .. })
            ),
            "{name}: a cursor naming no event"
        );
    }
}

/// Clearing kinds of mark the spool does not hold changes nothing.
#[test]
fn clearing_marks_the_spool_does_not_hold_changes_nothing() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let print = open(&port, "bracket.gcode");
        let spool = register(&port, "doubted", Some(0), 500.0, at(0));
        let marked = block_on(port.apply_spool_changes(
            vec![change(
                spool.id,
                Change::MarkUnverified {
                    reason: uncharged(print.id),
                },
                at(1),
            )],
            Vec::new(),
            Vec::new(),
        ))
        .expect("the mark applies");
        let untouched = block_on(port.apply_spool_changes(
            vec![change(
                spool.id,
                Change::ClearMarks {
                    kinds: vec![
                        UnverifiedReasonKind::TallyReset,
                        UnverifiedReasonKind::FilamentChangeUnconfirmed,
                    ],
                },
                at(9),
            )],
            Vec::new(),
            Vec::new(),
        ))
        .expect("the clearing answers");
        assert_eq!(untouched.events, Vec::new(), "{name}");
        assert_eq!(
            untouched.spools, marked.spools,
            "{name}: clearing marks the spool does not hold changed its trust or its instant"
        );
    }
}

/// Retiring a spool still on its tool frees the tool for another.
#[test]
fn retiring_a_loaded_spool_frees_its_tool() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let retiring = register(&port, "empty", Some(0), 2.0, at(0));
        let next = register(&port, "full", None, 1_000.0, at(1));
        block_on(port.apply_spool_changes(
            vec![change(retiring.id, Change::Retire, at(2))],
            Vec::new(),
            Vec::new(),
        ))
        .expect("the spool retires on its tool");
        assert_eq!(block_on(port.loaded_spools()), Ok(Vec::new()), "{name}");
        block_on(port.apply_spool_changes(
            vec![change(next.id, Change::Load { tool: 0 }, at(3))],
            Vec::new(),
            Vec::new(),
        ))
        .expect("the tool a retired spool held takes another");
        assert_eq!(
            block_on(port.loaded_spools())
                .map(|found| found.iter().map(|spool| spool.id).collect::<Vec<_>>()),
            Ok(vec![next.id]),
            "{name}"
        );
    }
}

/// A charge naming no spool is recorded uncharged and debits nothing, and one
/// whose calculation comes to no grams debits nothing either.
#[test]
fn a_charge_naming_no_spool_or_no_grams_is_recorded_and_debits_nothing() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let print = open(&port, "bracket.gcode");
        let spool = register(&port, "tracked", Some(1), 500.0, at(0));
        let write = block_on(port.apply_spool_changes(
            Vec::new(),
            vec![
                charge(print_key(print.id, 0, 0), Some(print.id), None, 0.0, at(5)),
                charge(
                    print_key(print.id, 1, 0),
                    Some(print.id),
                    Some(spool.id),
                    0.0,
                    at(5),
                ),
            ],
            Vec::new(),
        ))
        .expect("the charges apply");
        assert_eq!(
            kinds(&write.events),
            ["filament_consumed", "filament_consumed"],
            "{name}"
        );
        assert_eq!(
            block_on(port.charges(print.id)).map(|found| found
                .iter()
                .map(|record| (
                    record.spool_id(),
                    record.charged.as_ref().map(|charged| charged.grams)
                ))
                .collect::<Vec<_>>()),
            Ok(vec![(None, None), (Some(spool.id), Some(0.0))]),
            "{name}"
        );
        assert_grams(remaining(&port, spool.id), 500.0, name);
    }
}

/// A charge taken before the spool last changed leaves that instant alone.
#[test]
fn a_charge_older_than_the_spools_last_change_leaves_its_instant() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let print = open(&port, "bracket.gcode");
        let spool = register(&port, "corrected", Some(0), 500.0, at(0));
        block_on(port.apply_spool_changes(
            vec![change(
                spool.id,
                Change::SetRemaining { remaining_g: 400.0 },
                at(20),
            )],
            Vec::new(),
            Vec::new(),
        ))
        .expect("the correction applies");
        let write = block_on(port.apply_spool_changes(
            Vec::new(),
            vec![charge(
                print_key(print.id, 0, 0),
                Some(print.id),
                Some(spool.id),
                15.0,
                at(10),
            )],
            Vec::new(),
        ))
        .expect("the charge applies");
        assert_eq!(
            write.spools[0].updated_at,
            at(20),
            "{name}: a late charge moved the spool's instant back"
        );
        assert_grams(write.spools[0].remaining_g, 385.0, name);
    }
}

/// A print's charges read back by tool and then segment, under keys whose
/// free-text parts carry colons.
#[test]
fn a_prints_charges_read_by_tool_then_segment_under_every_key_spelling() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let print = open(&port, "bracket.gcode");
        let colons = printobserver_core::ChargeKey::Unobserved {
            origin: "sd:card".to_owned(),
            file_name: "parts:v2:bracket.gcode".to_owned(),
            generation: 12,
            print_number: 3,
            tool: -1,
        };
        let submitted = [
            print_key(print.id, 1, 0),
            print_key(print.id, 0, 10),
            colons.clone(),
            print_key(print.id, 0, 2),
            print_key(print.id, 1, 1),
        ];
        block_on(
            port.apply_spool_changes(
                Vec::new(),
                submitted
                    .iter()
                    .map(|key| charge(key.clone(), Some(print.id), None, 0.0, at(5)))
                    .collect(),
                Vec::new(),
            ),
        )
        .expect("the charges apply");
        assert_eq!(
            block_on(port.charges(print.id)).map(|found| found
                .into_iter()
                .map(|record| record.identity.key().clone())
                .collect::<Vec<_>>()),
            Ok(vec![
                colons.clone(),
                print_key(print.id, 0, 2),
                print_key(print.id, 0, 10),
                print_key(print.id, 1, 0),
                print_key(print.id, 1, 1),
            ]),
            "{name}: the charges are not by tool and then segment, or a key read back changed"
        );
        let replay = block_on(port.apply_spool_changes(
            Vec::new(),
            vec![charge(colons.clone(), Some(print.id), None, 0.0, at(6))],
            Vec::new(),
        ))
        .expect("the replay answers");
        assert_eq!(
            (
                replay.events.len(),
                replay.charges[0].identity.key().clone()
            ),
            (0, colons),
            "{name}: a key carrying colons was not found recorded"
        );
    }
}

/// The timeline breaks a tie of instants by identifier, follows a cursor
/// inside one instant, finds a spool however deep a payload names it, and
/// carries the image an event arrived with.
#[test]
fn the_timeline_breaks_ties_finds_a_spool_at_any_depth_and_carries_images() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let print = open(&port, "bracket.gcode");
        let (spool, _) = block_on(port.register_spool(
            new_spool("tracked", Some(0), 500.0, at(0)),
            operator_source(),
            Actor::Operator,
            "a new spool".to_owned(),
        ))
        .expect("the spool registers");
        let started = |minute| {
            block_on(port.append_event(event_of(
                Some(print.id),
                at(minute),
                &PrintStartedPayload {
                    file_name: None,
                    started_by: None,
                    loaded: vec![printobserver_core::LoadedSpool {
                        tool: 0,
                        spool_id: spool.id,
                    }],
                    preflight: None,
                },
            )))
            .expect("the start is appended")
        };
        let mut tied = vec![started(5), started(5)];
        tied.sort_by_key(|event| core::cmp::Reverse(event.id));
        let mentioned = block_on(port.append_event(event_of(
            None,
            at(6),
            &printobserver_core::SpoolUnloadedPayload {
                spool_id: SpoolId::new(),
                tool: 1,
                actor: Actor::Operator,
                reason: format!("the shelf beside {}", spool.id),
            },
        )))
        .expect("the unload is appended");
        let image = block_on(port.put_image(
            print.id,
            tied[0].id,
            None,
            "image/jpeg".to_owned(),
            printobserver_types::RawBytes::new(b"the first layer".to_vec()),
        ))
        .expect("an image is stored");

        let read = |query: TimelineQuery| block_on(port.timeline(query)).expect("it reads");
        let by_spool = read(TimelineQuery {
            spool_id: Some(spool.id),
            since: Some(at(5)),
            ..whole_timeline(None)
        });
        assert_eq!(
            ids(&by_spool),
            ids(&tied),
            "{name}: a spool named inside a list, or one instant's tie, was not read newest first"
        );
        assert!(
            !ids(&read(TimelineQuery {
                spool_id: Some(spool.id),
                ..whole_timeline(None)
            }))
            .contains(&mentioned.id),
            "{name}: a string merely containing the spool's identifier matched it"
        );
        assert_eq!(
            by_spool[0].image.as_ref().map(|carried| carried.id),
            Some(image.id),
            "{name}: the timeline lost the image its event arrived with"
        );
        assert_eq!(
            ids(&read(TimelineQuery {
                before: Some(tied[0].id),
                since: Some(at(5)),
                kinds: vec![EventKind::new(PrintStartedPayload::KIND).expect("a kind name")],
                ..whole_timeline(None)
            })),
            vec![tied[1].id],
            "{name}: the cursor did not follow inside one instant"
        );
    }
}

/// A charge or a pend refused after a charge has already taken a pending debit
/// in the same call leaves none of the call: not the debit, not the taking, not
/// the event.
#[test]
fn a_charge_or_pend_refused_late_in_a_call_rolls_back_the_taking_before_it() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let print = open(&port, "bracket.gcode");
        let ghost = PrintId::new();
        let spool = register(&port, "next", Some(0), 500.0, at(0));
        block_on(port.apply_spool_changes(
            Vec::new(),
            Vec::new(),
            vec![pend(segment_key(print.id, 0, 1), at(5))],
        ))
        .expect("the pend holds");
        let taking = || {
            charge(
                print_key(print.id, 0, 1),
                Some(print.id),
                Some(spool.id),
                5.0,
                at(10),
            )
        };
        // A charge against a print nothing opened, whose own event names none.
        let mut unopened = charge(
            print_key(ghost, 0, 0),
            Some(ghost),
            Some(spool.id),
            1.0,
            at(10),
        );
        unopened.event.print_id = None;
        let mut orphaned = pend(segment_key(ghost, 0, 1), at(10));
        orphaned.event.print_id = None;
        let attempts = vec![
            (
                vec![
                    taking(),
                    charge(
                        print_key(print.id, 0, 2),
                        Some(print.id),
                        Some(SpoolId::new()),
                        1.0,
                        at(10),
                    ),
                ],
                Vec::new(),
                "a spool nothing registered",
            ),
            (vec![taking(), unopened], Vec::new(), "charges.print_id"),
            (vec![taking()], vec![orphaned], "pending_charges.print_id"),
        ];
        let before = ledger_state(&port, &[print.id]);
        for (charges, pends, refusal) in attempts {
            let answer = block_on(port.apply_spool_changes(Vec::new(), charges, pends));
            match refusal {
                "a spool nothing registered" => assert!(
                    matches!(answer, Err(StoreError::NotFound { .. })),
                    "{name}: a charge naming {refusal} answered {answer:?}"
                ),
                constraint => assert_eq!(answer, Err(refused(constraint)), "{name}"),
            }
            assert_eq!(
                ledger_state(&port, &[print.id]),
                before,
                "{name}: refused on {refusal}, the call left its earlier taking behind"
            );
        }
    }
}

/// A correction in the same call as a charge taking a pending debit sets the
/// figure after that debit, so the correction is what the spool reads.
#[test]
fn a_correction_beside_a_taken_pending_charge_is_what_the_spool_reads() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let print = open(&port, "bracket.gcode");
        let spool = register(&port, "weighed", Some(0), 500.0, at(0));
        block_on(port.apply_spool_changes(
            Vec::new(),
            Vec::new(),
            vec![pend(segment_key(print.id, 0, 1), at(5))],
        ))
        .expect("the pend holds");
        let write = block_on(port.apply_spool_changes(
            vec![change(
                spool.id,
                Change::SetRemaining { remaining_g: 300.0 },
                at(10),
            )],
            vec![charge(
                print_key(print.id, 0, 1),
                Some(print.id),
                Some(spool.id),
                5.0,
                at(10),
            )],
            Vec::new(),
        ))
        .expect("the correction applies");
        assert_eq!(
            kinds(&write.events),
            ["filament_consumed", "spool_adjusted"]
        );
        assert_grams(remaining(&port, spool.id), 300.0, name);
        assert_eq!(
            block_on(port.pending_charges(None)),
            Ok(Vec::new()),
            "{name}"
        );
    }
}
