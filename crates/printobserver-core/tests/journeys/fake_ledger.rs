//! The fake store's filament ledger answers as `FilamentStore` states.
//!
//! The journeys that account filament run on the fake store, so its ledger is
//! held here to the rules the trait states — the replay rules, every refusal
//! leaving no partial write, which change writes an event, a settlement taken
//! once and the timeline's filters — through the same trait objects the
//! supervisor holds. The store crate's conformance suite holds its two
//! implementations to the same rules; this crate may depend on neither.

use std::sync::Arc;

use printobserver_core::block_on;
use printobserver_core::kinds::{
    ChargePendedPayload, FilamentConsumedPayload, PrintEndedPayload, SpoolRetiredPayload,
    SpoolUnverifiedPayload, operator_source, system_source,
};
use printobserver_core::store::{
    Charge, ChargeIdentity, EventDraft, NewSpool, Pend, Settlement, SpoolChange, SpoolMutation,
    StoreError, Stores, TimelineQuery,
};
use printobserver_core::{
    Actor, ChargeKey, ChargeMethod, ChargedFilament, FilePosition, PendingCharge, PrintOutcome,
    PrintSegmentKey, SpoolId, SpoolRecord, SpoolTrust, UnverifiedReason, UnverifiedReasonKind,
};
use printobserver_printer_api::PrinterState;
use printobserver_types::contract::Sample;
use printobserver_types::{EventBody, EventKind, EventPayload, EventRecord, PrintId};

use crate::fakes::{FakeClock, FakeStore};
use crate::journal::Journal;
use crate::world::at;

/// The fake store, behind the handles the supervisor holds.
fn stores() -> Stores {
    let clock = Arc::new(FakeClock::new(crate::world::EPOCH_SECONDS));
    Stores::of(Arc::new(FakeStore::new(
        Arc::new(Journal::default()),
        clock,
    )))
}

/// One event of a declared kind.
fn event_of<P: EventPayload>(print_id: Option<PrintId>, seconds: i64, payload: &P) -> EventDraft {
    EventDraft {
        print_id,
        source: system_source(),
        received_at: at(seconds),
        body: EventBody::of(payload).expect("a declared payload serializes"),
        raw: None,
    }
}

/// Register one spool.
fn register(stores: &Stores, loaded_on_tool: Option<i64>, seconds: i64) -> SpoolRecord {
    block_on(stores.filament.register_spool(
        NewSpool {
            name: "shelf PLA".to_owned(),
            material: "PLA".to_owned(),
            color: None,
            diameter_mm: 1.75,
            density_g_cm3: 1.24,
            initial_g: 1_000.0,
            remaining_g: 500.0,
            tare_g: None,
            loaded_on_tool,
            trust: SpoolTrust::Verified,
            registered_at: at(seconds),
        },
        operator_source(),
        Actor::Operator,
        "a new spool".to_owned(),
    ))
    .expect("the spool registers")
    .0
}

/// One change, carrying a stand-in event that names the spool when it
/// carries one.
fn change(
    spool_id: SpoolId,
    change: impl FnOnce(EventDraft) -> SpoolChange,
    seconds: i64,
) -> SpoolMutation {
    let event = event_of(
        None,
        seconds,
        &SpoolRetiredPayload {
            spool_id,
            remaining_g: 0.0,
            actor: Actor::Operator,
            reason: "a stand-in event for the change".to_owned(),
        },
    );
    SpoolMutation {
        spool_id,
        change: change(event),
        at: at(seconds),
    }
}

/// One mark, with its `spool_unverified` event.
fn marking(spool_id: SpoolId, reason: UnverifiedReason, seconds: i64) -> SpoolMutation {
    let event = event_of(
        None,
        seconds,
        &SpoolUnverifiedPayload {
            spool_id,
            reason: reason.clone(),
        },
    );
    SpoolMutation {
        spool_id,
        change: SpoolChange::MarkUnverified { reason, event },
        at: at(seconds),
    }
}

/// One debit of `grams` from a spool, under a watched print's segment.
fn charge(print_id: PrintId, segment: u32, spool_id: SpoolId, grams: f64) -> Charge {
    let key = ChargeKey::Print {
        print_id,
        tool: 0,
        segment,
    };
    let charged = ChargedFilament {
        spool_id,
        grams,
        diameter_mm: 1.75,
        density_g_cm3: 1.24,
        remaining_before_g: 500.0,
        remaining_after_g: 500.0 - grams,
    };
    Charge {
        event: event_of(
            Some(print_id),
            10,
            &FilamentConsumedPayload {
                charge_key: key.clone(),
                tool: 0,
                method: ChargeMethod::GcodeExtrusion,
                length_mm: 100.0,
                charged: Some(charged.clone()),
                analysed_length_mm: None,
                position: None,
                note: None,
            },
        ),
        identity: ChargeIdentity::of(key),
        length_mm: 100.0,
        charged: Some(charged),
        at: at(10),
    }
}

/// One charge held for a spool not yet named.
fn pend(print_id: PrintId, segment: u32, seconds: i64) -> Pend {
    let pending = PendingCharge {
        charge_key: PrintSegmentKey {
            print_id,
            tool: 0,
            segment,
        },
        length_mm: 40.0,
        method: ChargeMethod::GcodeExtrusion,
        analysed_length_mm: None,
        position: None,
        note: None,
        pended_at: at(seconds),
    };
    Pend {
        event: event_of(
            Some(print_id),
            seconds,
            &ChargePendedPayload {
                pending: pending.clone(),
            },
        ),
        pending,
    }
}

/// A timeline read with no filter.
const fn everything() -> TimelineQuery {
    TimelineQuery {
        kinds: Vec::new(),
        since: None,
        until: None,
        spool_id: None,
        before: None,
        limit: None,
    }
}

/// The kinds of some events, in order.
fn kinds(events: &[EventRecord]) -> Vec<&str> {
    events.iter().map(|event| event.kind().as_str()).collect()
}

/// A spool's remaining figure.
fn remaining(stores: &Stores, spool_id: SpoolId) -> f64 {
    block_on(stores.filament.spool(spool_id))
        .expect("the spool reads")
        .expect("it exists")
        .remaining_g
}

/// A refusal naming one constraint.
fn refused(constraint: &str) -> StoreError {
    StoreError::ConstraintRefused {
        constraint: constraint.to_owned(),
    }
}

/// A charge is debited once, a pend is taken once, and a refusal writes none
/// of its call.
#[test]
fn the_fake_ledger_debits_once_and_refuses_whole_calls() {
    let stores = stores();
    let print = block_on(stores.prints.open_print(None, None)).expect("a print opens");
    let held = register(&stores, Some(0), 0);
    let spare = register(&stores, None, 1);

    block_on(stores.filament.apply_spool_changes(
        Vec::new(),
        Vec::new(),
        vec![pend(print.id, 1, 5)],
    ))
    .expect("the pend holds");
    let first = block_on(stores.filament.apply_spool_changes(
        Vec::new(),
        vec![
            charge(print.id, 0, held.id, 25.0),
            charge(print.id, 1, held.id, 5.0),
        ],
        vec![pend(print.id, 1, 6)],
    ))
    .expect("the charges apply");
    assert_eq!(
        kinds(&first.events),
        ["filament_consumed", "filament_consumed"]
    );
    assert_eq!(
        block_on(stores.filament.pending_charges(Some(0))),
        Ok(Vec::new())
    );
    let again = block_on(stores.filament.apply_spool_changes(
        Vec::new(),
        vec![charge(print.id, 0, held.id, 25.0)],
        Vec::new(),
    ))
    .expect("the replay answers");
    assert_eq!(
        (again.events.len(), again.charges),
        (0, vec![first.charges[0].clone()])
    );
    assert!((remaining(&stores, held.id) - 470.0).abs() < 1e-9);

    let before = block_on(stores.timeline.timeline(everything())).expect("it reads");
    assert_eq!(
        block_on(stores.filament.apply_spool_changes(
            vec![change(
                spare.id,
                |event| SpoolChange::Load { tool: 0, event },
                20
            )],
            vec![charge(print.id, 2, held.id, 1.0)],
            Vec::new(),
        )),
        Err(refused("spools.loaded_on_tool"))
    );
    assert_eq!(
        block_on(stores.timeline.timeline(everything())),
        Ok(before),
        "a refused call wrote its charge"
    );
    assert!((remaining(&stores, held.id) - 470.0).abs() < 1e-9);
    assert_eq!(
        block_on(stores.filament.charges(print.id)).map(|found| found.len()),
        Ok(2)
    );
}

/// Marks are kept per reason, a retired spool refuses all but a mark, and a
/// print is settled once.
#[test]
fn the_fake_ledger_keeps_marks_refuses_a_retired_spool_and_settles_once() {
    let stores = stores();
    let print = block_on(stores.prints.open_print(None, None)).expect("a print opens");
    let spool = register(&stores, Some(0), 0);
    let mark = |reason| marking(spool.id, reason, 3);
    let uncharged = UnverifiedReason::UnchargedPrint { print_id: print.id };
    let unconfirmed = UnverifiedReason::FilamentChangeUnconfirmed { print_id: print.id };

    let marked = block_on(stores.filament.apply_spool_changes(
        vec![mark(uncharged.clone()), mark(unconfirmed), mark(uncharged)],
        Vec::new(),
        Vec::new(),
    ))
    .expect("the marks apply");
    assert_eq!(
        kinds(&marked.events),
        ["spool_unverified", "spool_unverified"]
    );
    let cleared = block_on(stores.filament.apply_spool_changes(
        vec![change(
            spool.id,
            |_| SpoolChange::ClearMarks {
                kinds: vec![UnverifiedReasonKind::UnchargedPrint],
            },
            4,
        )],
        Vec::new(),
        Vec::new(),
    ))
    .expect("the clearing applies");
    assert_eq!(cleared.events, Vec::new(), "clearing wrote an event");
    assert!(
        matches!(&cleared.spools[0].trust, SpoolTrust::Unverified { marks } if marks.len() == 1)
    );

    block_on(stores.filament.apply_spool_changes(
        vec![change(spool.id, |event| SpoolChange::Retire { event }, 5)],
        Vec::new(),
        Vec::new(),
    ))
    .expect("the spool retires");
    assert_eq!(
        block_on(stores.filament.apply_spool_changes(
            vec![change(
                spool.id,
                |event| SpoolChange::SetRemaining {
                    remaining_g: 9.0,
                    event,
                },
                6
            )],
            Vec::new(),
            Vec::new(),
        )),
        Err(refused("spools.retired_at"))
    );

    let ended = event_of(
        Some(print.id),
        30,
        &PrintEndedPayload {
            outcome: PrintOutcome::Completed,
            state: PrinterState::Operational,
            reason: "finished".to_owned(),
            last_position: None,
        },
    );
    let settle = || {
        block_on(stores.filament.settle_print(
            print.id,
            PrintOutcome::Completed,
            ended.clone(),
            vec![charge(print.id, 0, spool.id, 2.0)],
            vec![pend(print.id, 1, 30)],
            Vec::new(),
        ))
    };
    let Ok(Settlement::Settled { events, .. }) = settle() else {
        panic!("the first settlement did not settle");
    };
    assert_eq!(
        kinds(&events),
        ["filament_consumed", "charge_pended", "print_ended"]
    );
    assert_eq!(
        settle(),
        Ok(Settlement::AlreadySettled {
            at: at(30),
            outcome: PrintOutcome::Completed,
        })
    );
    assert_eq!(
        block_on(stores.filament.print_settlement(print.id)),
        Ok(Some((at(30), PrintOutcome::Completed)))
    );
}

/// Tallies keep their generation, positions keep the latest, and the timeline
/// answers its kinds alone.
#[test]
fn the_fake_ledger_keeps_tallies_and_positions_and_reads_the_timeline() {
    let stores = stores();
    let print = block_on(stores.prints.open_print(None, None)).expect("a print opens");
    let spool = register(&stores, Some(0), 0);
    let tally = |generation, seconds| printobserver_core::store::FileTally {
        file_name: "bracket.gcode".to_owned(),
        origin: "local".to_owned(),
        generation,
        successes: 1,
        failures: 0,
        observed_at: at(seconds),
    };
    block_on(stores.filament.account_tallies(
        vec![tally(2, 1)],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    ))
    .expect("the tally accounts");
    assert_eq!(
        block_on(stores.filament.account_tallies(
            vec![tally(1, 2)],
            vec![charge(print.id, 0, spool.id, 3.0)],
            Vec::new(),
            Vec::new(),
        )),
        Err(refused("file_tallies.generation"))
    );
    assert_eq!(
        block_on(stores.filament.file_tallies()),
        Ok(vec![tally(2, 1)])
    );
    assert_eq!(block_on(stores.filament.charges(print.id)), Ok(Vec::new()));

    let position = |bytes, seconds| FilePosition {
        bytes,
        size_bytes: None,
        observed_at: at(seconds),
    };
    block_on(
        stores
            .filament
            .record_file_position(print.id, position(9, 9)),
    )
    .expect("recorded");
    block_on(
        stores
            .filament
            .record_file_position(print.id, position(5, 5)),
    )
    .expect("recorded");
    assert_eq!(
        block_on(stores.filament.file_position(print.id)),
        Ok(Some(position(9, 9)))
    );

    block_on(stores.events.append_event(event_of(
        Some(print.id),
        40,
        &printobserver_core::PortFailurePayload::sample_full(),
    )))
    .expect("a non-timeline event is appended");
    let timeline = block_on(stores.timeline.timeline(everything())).expect("it reads");
    assert_eq!(kinds(&timeline), ["spool_registered"]);
    assert_eq!(
        block_on(stores.timeline.timeline(TimelineQuery {
            spool_id: Some(SpoolId::new()),
            ..everything()
        })),
        Ok(Vec::new())
    );
    assert_eq!(
        block_on(stores.timeline.timeline(TimelineQuery {
            kinds: vec![EventKind::new("port_failure").expect("a kind name")],
            ..everything()
        })),
        Err(refused("timeline.kinds"))
    );
    assert_eq!(
        block_on(stores.filament.loaded_spools()).map(|found| found.len()),
        Ok(1)
    );
    assert_eq!(
        block_on(stores.filament.spools(false)).map(|found| found.len()),
        Ok(1)
    );
}
