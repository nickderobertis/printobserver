//! A filament change the printer signals charges the spool up to where it
//! stopped, and holds what follows until somebody says which spool fed it.
//!
//! The signal arrives as the alert the Obico adapter normalizes a
//! `filament_change` notification to — read from the fixture that adapter's
//! own test pins its output to — so each journey here runs from the body Obico
//! posts, through the supervisor, to the timeline and the spool records.

use std::sync::Arc;

use printobserver_core::kinds::{
    ChargePendedPayload, FilamentChangeSignalledPayload, FilamentConsumedPayload,
    PrintEndedPayload, SpoolAdjustedPayload, SpoolLoadedPayload,
};
use printobserver_core::store::StoreError;
use printobserver_core::{
    ChargeKey, ChargeMethod, ClearedMark, Clock as _, PolicyDecision, PreflightMode,
    RejectionReason, SpoolCommand, SpoolId, SpoolRecord, SpoolTrust, Supervisor, UnverifiedReason,
    UnverifiedReasonKind, block_on,
};
use printobserver_printer_api::{PrinterError, PrinterState};
use printobserver_types::PrintId;
use printobserver_vision_api::{NormalizedAlert, PrinterSignal};

use crate::fakes::{PrinterMethod, StoreMethod};
use crate::shelf::{
    FILE, SIZE, TOTAL_MM, adjust_command, after_an_uncharged_print, assert_close,
    assert_ledger_explains, charges_to, command, command_on, ends, ends_on, every, every_of, grams,
    load, load_command, loaded_spool, on_tool, paused_at, register, request_start, running_at,
    running_at_on, spool_now, spool_of, spool_world_with, start,
};
use crate::world::World;

/// Where the print had got at the last periodic read before the change.
const PERIODIC_AT: i64 = 100_000;

/// What tool 0 had fed by then.
const PERIODIC_MM: f64 = 500.0;

/// Where the printer paused for the change.
const STOPPED_AT: i64 = 300_000;

/// What tool 0 had fed by then.
const STOPPED_MM: f64 = 1_500.0;

/// The alert the Obico adapter normalizes a `filament_change` notification to,
/// exactly as its own test pins it, received now.
fn obico_filament_change(world: &World) -> NormalizedAlert {
    let mut alert: NormalizedAlert = printobserver_types::serde_json::from_str(include_str!(
        "../../../printobserver-obico/tests/fixtures/filament-change-alert.json"
    ))
    .expect("the committed alert reads");
    alert.received_at = world.clock.now();
    alert
}

/// One print at the point a filament change was signalled for it.
struct Change {
    /// The world it runs in.
    world: World,
    /// The spool that was loaded as the change was signalled.
    spool: SpoolRecord,
    /// The print.
    print_id: PrintId,
}

impl Change {
    /// The spool as it stands now.
    fn spool(&self) -> SpoolRecord {
        spool_now(&self.world, self.spool.id)
    }

    /// The reasons the spool's figure is in doubt now, oldest first.
    fn marks(&self) -> Vec<UnverifiedReason> {
        reasons(&self.spool())
    }

    /// Every charge made for the print on tool 0.
    fn charges(&self) -> Vec<FilamentConsumedPayload> {
        every_of::<FilamentConsumedPayload>(&self.world, self.print_id)
            .into_iter()
            .filter(|charge| charge.tool == 0)
            .collect()
    }

    /// Every length charged or held for the print on tool 0.
    fn accounted_mm(&self) -> f64 {
        let charged: f64 = self.charges().iter().map(|charge| charge.length_mm).sum();
        let held: f64 = pending(&self.world)
            .iter()
            .filter(|held| held.print_id() == self.print_id)
            .map(|held| held.length_mm)
            .sum();
        charged + held
    }

    /// The filament-change mark the signal left, by whether its read worked.
    const fn change_mark(&self, read_ok: bool) -> UnverifiedReason {
        if read_ok {
            UnverifiedReason::FilamentChangeUnconfirmed {
                print_id: self.print_id,
            }
        } else {
            UnverifiedReason::FilamentChangeUnconfirmedPosition {
                print_id: self.print_id,
            }
        }
    }

    /// The print completes and is closed out.
    fn completes(&self) {
        running_at(&self.world, SIZE);
        ends(&self.world, PrinterState::Operational);
    }
}

/// The reasons a spool's figure is in doubt, oldest first.
fn reasons(spool: &SpoolRecord) -> Vec<UnverifiedReason> {
    match &spool.trust {
        SpoolTrust::Verified => Vec::new(),
        SpoolTrust::Unverified { marks } => marks.iter().map(|mark| mark.reason.clone()).collect(),
    }
}

/// Every charge held after an unconfirmed change.
fn pending(world: &World) -> Vec<printobserver_core::PendingCharge> {
    block_on(world.core.pending_charges(None)).expect("the pending charges are read")
}

/// A print of the file, on a spool already holding an `uncharged_print` mark
/// when asked, with the printer paused at the change and the Obico alert for
/// it handled — its job read at the signal working or failing.
///
/// Starts are checked strictly, unless the spool is to start in doubt: then
/// the check only warns, so that the print starts at all.
fn signalled(read_ok: bool, prior_mark: bool) -> Change {
    let world = spool_world_with(|config| {
        if prior_mark {
            config.spools.preflight = PreflightMode::Warn;
        }
    });
    // The alert names the print Obico saw start, so the clock stands after it.
    world.clock.advance(80_000_000);
    let spool = loaded_spool(&world, "Before The Change", 1_000.0);
    if prior_mark {
        after_an_uncharged_print(&world);
    }
    let print_id = start(&world);
    world
        .printer
        .extrudes(FILE, PERIODIC_AT, vec![on_tool(0, PERIODIC_MM)]);
    world
        .printer
        .extrudes(FILE, STOPPED_AT, vec![on_tool(0, STOPPED_MM)]);
    running_at(&world, PERIODIC_AT);
    paused_at(&world, STOPPED_AT);
    if !read_ok {
        world.printer.fails(
            PrinterMethod::Job,
            PrinterError::Unreachable {
                detail: "the printer is busy changing filament".to_owned(),
            },
        );
    }
    world
        .handle(obico_filament_change(&world))
        .expect("the alert is handled");
    world.printer.heals();
    Change {
        world,
        spool,
        print_id,
    }
}

/// The charge made for one segment of a print on tool 0.
fn segment(change: &Change, segment: u32) -> FilamentConsumedPayload {
    let key = ChargeKey::Print {
        print_id: change.print_id,
        tool: 0,
        segment,
    };
    change
        .charges()
        .into_iter()
        .find(|charge| charge.charge_key == key)
        .unwrap_or_else(|| panic!("segment {segment} was charged"))
}

/// A `filament_change` notification from Obico reaching the supervisor reads
/// the job afresh and charges the loaded spool up to where the printer stopped
/// — not to the older periodic position — writes the signal with that position
/// read at it, and marks the spool as one whose change nobody confirmed.
#[test]
fn an_obico_filament_change_charges_the_spool_to_the_stop_point_read_at_the_signal() {
    let change = signalled(true, false);
    let world = &change.world;
    assert_eq!(
        obico_filament_change(world).signal,
        Some(PrinterSignal::FilamentChange)
    );

    let signals = every::<FilamentChangeSignalledPayload>(world);
    assert_eq!(signals.len(), 1);
    assert_eq!(signals[0].tool, Some(0));
    assert_eq!(signals[0].spool_id, Some(change.spool.id));
    assert_eq!(
        signals[0].source_kind.as_str(),
        "obico_printer_notification"
    );
    assert!(
        signals[0]
            .position
            .as_ref()
            .is_some_and(|signal| signal.read_at_signal)
    );
    assert_eq!(
        signals[0]
            .position
            .as_ref()
            .map(|signal| signal.position.bytes),
        Some(STOPPED_AT)
    );
    let charges = change.charges();
    assert_eq!(charges.len(), 1);
    assert_eq!(spool_of(&charges[0]), Some(change.spool.id));
    assert_eq!(charges[0].method, ChargeMethod::GcodeExtrusion);
    assert_close(charges[0].length_mm, STOPPED_MM);
    assert_eq!(
        charges[0].position.as_ref().map(|position| position.bytes),
        Some(STOPPED_AT)
    );
    assert_eq!(change.marks(), vec![change.change_mark(true)]);
    assert_close(
        change.spool().remaining_g,
        1_000.0 - grams(&change.spool, STOPPED_MM),
    );
    world.journal.assert_no_violations();
}

/// With no load, the print's remainder after the change is held as one pending
/// charge rather than charged to the spool still recorded on the tool; a later
/// load of another spool takes it, by its own diameter and density, names it,
/// empties the pending charges, clears the outgoing spool's mark and leaves the
/// incoming spool's trust as it was.
#[test]
fn the_remainder_after_an_unconfirmed_change_is_held_and_the_next_load_takes_it() {
    let change = signalled(true, false);
    let world = &change.world;
    change.completes();

    let pended = every::<ChargePendedPayload>(world);
    assert_eq!(pended.len(), 1);
    let held = &pended[0].pending;
    assert_close(held.length_mm, TOTAL_MM - STOPPED_MM);
    assert_eq!(held.method, ChargeMethod::SlicerEstimate);
    assert_eq!(
        held.charge_key.charge_key(),
        ChargeKey::Print {
            print_id: change.print_id,
            tool: 0,
            segment: 1
        }
    );
    assert_eq!(pending(world), vec![held.clone()]);
    assert_eq!(charges_to(world, change.spool.id).len(), 1);
    assert_close(
        change.spool().remaining_g,
        1_000.0 - grams(&change.spool, STOPPED_MM),
    );
    assert_eq!(change.marks(), vec![change.change_mark(true)]);
    assert_eq!(
        every_of::<PrintEndedPayload>(world, change.print_id).len(),
        1
    );

    let thick = command(
        world,
        SpoolCommand::RegisterSpool {
            name: "Thick PETG".to_owned(),
            material: "PETG".to_owned(),
            color: None,
            diameter_mm: Some(2.85),
            density_g_cm3: None,
            initial_g: 1_000.0,
            remaining_g: None,
            tare_g: None,
            reason: "the replacement".to_owned(),
            actor: printobserver_core::Actor::Operator,
        },
    )
    .spool
    .expect("registered");
    let trust_before = thick.trust.clone();
    let loaded = load(world, thick.id, 0);
    assert_eq!(loaded.decision, PolicyDecision::Accepted);

    let taken = charges_to(world, thick.id);
    assert_eq!(taken.len(), 1);
    assert_eq!(taken[0].charge_key, held.charge_key.charge_key());
    assert_close(taken[0].length_mm, TOTAL_MM - STOPPED_MM);
    assert_close(
        taken[0].charged.as_ref().expect("weighed").grams,
        grams(&thick, TOTAL_MM - STOPPED_MM),
    );
    assert_eq!(
        taken[0].charged.as_ref().map(|charged| charged.diameter_mm),
        Some(2.85)
    );
    assert!(
        taken[0]
            .note
            .as_deref()
            .is_some_and(|note| note.contains("filament change")),
        "{:?}",
        taken[0].note
    );
    let event = every::<SpoolLoadedPayload>(world)
        .pop()
        .expect("the load is written down");
    assert_eq!(event.took_pending.len(), 1);
    assert_eq!(
        event.took_pending[0].charge_key,
        held.charge_key.charge_key()
    );
    assert_eq!(event.replaced, Some(change.spool.id));
    assert_eq!(
        event.cleared_marks,
        vec![ClearedMark {
            spool_id: change.spool.id,
            reason: change.change_mark(true),
        }]
    );
    assert!(pending(world).is_empty());
    assert_eq!(change.spool().trust, SpoolTrust::Verified);
    assert_eq!(spool_now(world, thick.id).trust, trust_before);
    assert_close(change.accounted_mm(), TOTAL_MM);
    assert_ledger_explains(world, change.spool.id);
    assert_ledger_explains(world, thick.id);
}

/// A load during the print takes the filament fed since the signal up to the
/// position read then, and the print goes on charging the spool loaded.
#[test]
fn a_load_during_the_print_takes_what_was_fed_since_the_signal() {
    let change = signalled(true, false);
    let world = &change.world;
    let next = register(world, "Loaded Mid-Print", 1_000.0);
    running_at(world, 600_000);
    world
        .printer
        .extrudes(FILE, 600_000, vec![on_tool(0, 3_000.0)]);
    assert_eq!(load(world, next.id, 0).decision, PolicyDecision::Accepted);
    change.completes();

    let to_next = charges_to(world, next.id);
    assert_eq!(to_next.len(), 2);
    assert_close(to_next[0].length_mm, 3_000.0 - STOPPED_MM);
    assert_eq!(spool_of(&segment(&change, 1)), Some(next.id));
    assert_close(to_next[1].length_mm, TOTAL_MM - 3_000.0);
    assert_eq!(spool_of(&segment(&change, 2)), Some(next.id));
    assert!(every::<ChargePendedPayload>(world).is_empty());
    assert_eq!(charges_to(world, change.spool.id).len(), 1);
    assert_close(change.accounted_mm(), TOTAL_MM);
    assert_eq!(change.spool().trust, SpoolTrust::Verified);
    assert_ledger_explains(world, next.id);
}

/// Two signals before any load hold two segments, and the next load takes
/// both together.
#[test]
fn two_signals_before_a_load_hold_two_segments_the_next_load_takes_together() {
    let change = signalled(true, false);
    let world = &change.world;
    world
        .printer
        .extrudes(FILE, 600_000, vec![on_tool(0, 3_000.0)]);
    paused_at(world, 600_000);
    world
        .handle(obico_filament_change(world))
        .expect("the second alert is handled");
    change.completes();

    let pended = every::<ChargePendedPayload>(world);
    assert_eq!(pended.len(), 2);
    assert_close(pended[0].pending.length_mm, 3_000.0 - STOPPED_MM);
    assert_close(pended[1].pending.length_mm, TOTAL_MM - 3_000.0);
    assert_eq!(charges_to(world, change.spool.id).len(), 1);

    let next = register(world, "Takes Both", 1_000.0);
    let loaded = load(world, next.id, 0);
    assert_eq!(loaded.decision, PolicyDecision::Accepted);
    let event = every::<SpoolLoadedPayload>(world).pop().expect("written");
    assert_eq!(event.took_pending.len(), 2);
    let taken: f64 = charges_to(world, next.id)
        .iter()
        .map(|charge| charge.length_mm)
        .sum();
    assert_close(taken, TOTAL_MM - STOPPED_MM);
    assert!(pending(world).is_empty());
    assert_close(change.accounted_mm(), TOTAL_MM);
}

/// Saying the spool was never swapped — a correction of it, or a load naming
/// it again — sends what was held to it, so every bit of the print's filament
/// on the tool falls on that one spool, and clears its mark; and a load of the
/// spool already on a tool with nothing to confirm is refused.
#[test]
fn saying_the_spool_was_never_swapped_charges_it_what_was_held() {
    for by_adjusting in [true, false] {
        let change = signalled(true, false);
        let world = &change.world;
        change.completes();

        let answered = if by_adjusting {
            command(world, adjust_command(change.spool.id, 700.0))
        } else {
            load(world, change.spool.id, 0)
        };
        assert_eq!(answered.decision, PolicyDecision::Accepted);

        let charges = charges_to(world, change.spool.id);
        assert_eq!(charges.len(), 2);
        assert!(
            change
                .charges()
                .iter()
                .all(|charge| spool_of(charge) == Some(change.spool.id))
        );
        assert_close(change.accounted_mm(), TOTAL_MM);
        assert!(pending(world).is_empty());
        let cleared = if by_adjusting {
            let event = every::<SpoolAdjustedPayload>(world).pop().expect("written");
            assert_eq!(event.took_pending.len(), 1);
            assert_close(
                event.remaining_before_g,
                1_000.0 - grams(&change.spool, TOTAL_MM),
            );
            event.cleared_marks
        } else {
            let event = every::<SpoolLoadedPayload>(world).pop().expect("written");
            assert_eq!(event.took_pending.len(), 1);
            assert_eq!(event.replaced, None);
            event.cleared_marks
        };
        assert_eq!(
            cleared,
            vec![ClearedMark {
                spool_id: change.spool.id,
                reason: change.change_mark(true),
            }]
        );
        assert_eq!(change.spool().trust, SpoolTrust::Verified);
        assert_ledger_explains(world, change.spool.id);

        let again = load(world, change.spool.id, 0);
        assert!(
            matches!(
                again.decision,
                PolicyDecision::Rejected(RejectionReason::SpoolRefused { .. })
            ),
            "a load with nothing to confirm was not refused: {:?}",
            again.decision
        );
    }
}

/// When the job cannot be read at the signal, the spool is charged to the last
/// position recorded, the signal says the position was not read at it, and the
/// mark says the figure may be overstated: a load of another spool leaves that
/// mark — so a start on the old spool is still refused as unverified — while a
/// load naming the same spool takes what was held and clears it, and so does
/// a correction.
#[test]
fn a_change_whose_position_could_not_be_read_charges_the_last_position_and_says_so() {
    let change = signalled(false, false);
    let world = &change.world;
    let signals = every::<FilamentChangeSignalledPayload>(world);
    assert_eq!(signals.len(), 1);
    assert!(
        signals[0]
            .position
            .as_ref()
            .is_some_and(|signal| !signal.read_at_signal)
    );
    assert_eq!(
        signals[0]
            .position
            .as_ref()
            .map(|signal| signal.position.bytes),
        Some(PERIODIC_AT)
    );
    assert_close(segment(&change, 0).length_mm, PERIODIC_MM);
    assert_eq!(change.marks(), vec![change.change_mark(false)]);
    change.completes();
    assert_close(pending(world)[0].length_mm, TOTAL_MM - PERIODIC_MM);

    // Another spool takes what was held, and the doubt over the old one stays.
    let next = register(world, "Next", 1_000.0);
    let loaded = load(world, next.id, 0);
    assert_eq!(loaded.decision, PolicyDecision::Accepted);
    let event = every::<SpoolLoadedPayload>(world).pop().expect("written");
    assert!(event.cleared_marks.is_empty());
    assert_eq!(change.marks(), vec![change.change_mark(false)]);
    assert_close(change.accounted_mm(), TOTAL_MM);
    assert_eq!(
        load(world, change.spool.id, 0).decision,
        PolicyDecision::Accepted
    );
    let refused = request_start(world, printobserver_core::Actor::Operator, false);
    assert_eq!(
        refused.record.decision,
        PolicyDecision::Rejected(RejectionReason::UnverifiedSpool {
            tool: 0,
            spool_id: change.spool.id,
            spool_name: change.spool.name.clone(),
            reason: change.change_mark(false),
        })
    );

    // The same spool, named again, takes what was held and clears the doubt.
    let change = signalled(false, false);
    let world = &change.world;
    change.completes();
    assert_eq!(
        load(world, change.spool.id, 0).decision,
        PolicyDecision::Accepted
    );
    let event = every::<SpoolLoadedPayload>(world).pop().expect("written");
    assert_eq!(event.took_pending.len(), 1);
    assert_eq!(
        event.cleared_marks,
        vec![ClearedMark {
            spool_id: change.spool.id,
            reason: change.change_mark(false),
        }]
    );
    assert_eq!(change.spool().trust, SpoolTrust::Verified);
    assert_close(change.accounted_mm(), TOTAL_MM);

    // A correction clears it too.
    let change = signalled(false, false);
    let world = &change.world;
    change.completes();
    let adjusted = command(world, adjust_command(change.spool.id, 650.0));
    assert_eq!(adjusted.decision, PolicyDecision::Accepted);
    assert_eq!(change.spool().trust, SpoolTrust::Verified);
    let event = every::<SpoolAdjustedPayload>(world).pop().expect("written");
    assert_eq!(
        event.cleared_marks,
        vec![ClearedMark {
            spool_id: change.spool.id,
            reason: change.change_mark(false),
        }]
    );
}

/// A spool already holding an `uncharged_print` mark when a change is
/// signalled holds both; a load of another spool clears only the
/// filament-change one, and a correction clears both — each clearing named.
#[test]
fn a_change_on_a_spool_already_in_doubt_keeps_every_other_mark() {
    let change = signalled(true, true);
    let world = &change.world;
    let marks = change.marks();
    assert_eq!(marks.len(), 2);
    assert_eq!(marks[0].kind(), UnverifiedReasonKind::UnchargedPrint);
    assert_eq!(marks[1], change.change_mark(true));

    let next = register(world, "Swapped In", 1_000.0);
    assert_eq!(load(world, next.id, 0).decision, PolicyDecision::Accepted);
    let event = every::<SpoolLoadedPayload>(world).pop().expect("written");
    assert_eq!(
        event.cleared_marks,
        vec![ClearedMark {
            spool_id: change.spool.id,
            reason: change.change_mark(true),
        }]
    );
    let left = change.marks();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].kind(), UnverifiedReasonKind::UnchargedPrint);

    let change = signalled(true, true);
    let world = &change.world;
    let both = change.marks();
    let adjusted = command(world, adjust_command(change.spool.id, 600.0));
    assert_eq!(adjusted.decision, PolicyDecision::Accepted);
    let event = every::<SpoolAdjustedPayload>(world).pop().expect("written");
    assert_eq!(
        event.cleared_marks,
        both.into_iter()
            .map(|reason| ClearedMark {
                spool_id: change.spool.id,
                reason,
            })
            .collect::<Vec<_>>()
    );
    assert_eq!(change.spool().trust, SpoolTrust::Verified);
}

/// The same spool confirmed in place during the print, on a spool already
/// holding an `uncharged_print` mark — the read at the signal working or
/// failing, and the tool feeding or not between the signal and the
/// confirmation — is accepted, charges nothing at that moment, clears exactly
/// the one filament-change mark, and leaves the close-out of a supervisor
/// built afresh to charge that spool the remainder once.
#[test]
fn confirming_the_spool_in_place_mid_print_charges_it_the_remainder_at_close_out() {
    for read_ok in [true, false] {
        for fed_since in [true, false] {
            confirm_in_place(read_ok, fed_since);
        }
    }
}

/// One case of the confirmation in place.
fn confirm_in_place(read_ok: bool, fed_since: bool) {
    let case = format!("read_ok={read_ok}, fed_since={fed_since}");
    let change = signalled(read_ok, true);
    let world = &change.world;
    let charged_to_signal = if read_ok { STOPPED_MM } else { PERIODIC_MM };
    assert_close(segment(&change, 0).length_mm, charged_to_signal);
    if fed_since {
        running_at(world, 700_000);
    }
    let charges_before = every::<FilamentConsumedPayload>(world).len();

    let confirmed = load(world, change.spool.id, 0);
    assert_eq!(confirmed.decision, PolicyDecision::Accepted, "{case}");
    assert_eq!(
        every::<FilamentConsumedPayload>(world).len(),
        charges_before,
        "{case}: the confirmation charged something"
    );
    let event = every::<SpoolLoadedPayload>(world).pop().expect("written");
    assert_eq!(
        event.cleared_marks,
        vec![ClearedMark {
            spool_id: change.spool.id,
            reason: change.change_mark(read_ok),
        }],
        "{case}"
    );
    assert!(event.took_pending.is_empty(), "{case}");
    let left = change.marks();
    assert_eq!(left.len(), 1, "{case}");
    assert_eq!(
        left[0].kind(),
        UnverifiedReasonKind::UnchargedPrint,
        "{case}"
    );

    let restarted = world.restarted();
    running_at_on(world, &restarted, SIZE);
    world.store.fails(
        StoreMethod::EndPrint,
        StoreError::Database {
            detail: "the disk went away".to_owned(),
        },
    );
    world
        .printer
        .reports_job(Some(FILE), PrinterState::Operational);
    block_on(restarted.list_and_adopt_prints()).expect_err("the end could not be written");
    world.store.heals();
    let figure = change.spool().remaining_g;
    ends_on(world, &restarted, PrinterState::Operational);
    ends_on(world, &restarted, PrinterState::Operational);

    assert!(every::<ChargePendedPayload>(world).is_empty(), "{case}");
    assert!(
        block_on(restarted.pending_charges(None))
            .expect("read")
            .is_empty(),
        "{case}"
    );
    let charges = change.charges();
    assert_eq!(charges.len(), 2, "{case}");
    assert!(
        charges
            .iter()
            .all(|charge| spool_of(charge) == Some(change.spool.id) && charge.length_mm > 0.0),
        "{case}"
    );
    assert_close(segment(&change, 1).length_mm, TOTAL_MM - charged_to_signal);
    assert_close(change.accounted_mm(), TOTAL_MM);
    assert_close(change.spool().remaining_g, figure);
    assert_eq!(
        every_of::<PrintEndedPayload>(world, change.print_id).len(),
        1,
        "{case}"
    );
    let left = change.marks();
    assert_eq!(left.len(), 1, "{case}");
    assert_eq!(
        left[0].kind(),
        UnverifiedReasonKind::UnchargedPrint,
        "{case}"
    );
    assert_ledger_explains(world, change.spool.id);
}

/// A close-out that holds a remainder, repeated after its write committed and
/// its answer was lost — and, separately, after the write was refused — holds
/// it once.
#[test]
fn a_close_out_that_holds_a_remainder_holds_it_once_whatever_was_replayed() {
    for lost in [true, false] {
        let change = signalled(true, false);
        let world = &change.world;
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
        running_at(world, SIZE);
        world
            .printer
            .reports_job(Some(FILE), PrinterState::Operational);
        block_on(world.core.list_and_adopt_prints()).expect_err("the settlement failed");
        world.store.heals();
        ends(world, PrinterState::Operational);

        assert_eq!(pending(world).len(), 1, "lost={lost}");
        assert_eq!(every::<ChargePendedPayload>(world).len(), 1, "lost={lost}");
        assert_eq!(
            every_of::<PrintEndedPayload>(world, change.print_id).len(),
            1
        );
        assert_close(change.accounted_mm(), TOTAL_MM);
    }
}

/// A load that takes held charges, repeated after its write committed and its
/// answer was lost, debits the incoming spool once, one charge per held key.
#[test]
fn a_load_that_takes_held_charges_debits_once_when_replayed() {
    let change = signalled(true, false);
    let world = &change.world;
    change.completes();
    let next = register(world, "Replayed Load", 1_000.0);
    world.store.loses_the_answer_to(
        StoreMethod::ApplySpoolChanges,
        StoreError::Database {
            detail: "the connection dropped".to_owned(),
        },
    );
    block_on(world.core.request_spool_command(load_command(next.id, 0)))
        .expect_err("the answer was lost");
    let replay = load(world, next.id, 0);
    assert!(matches!(
        replay.decision,
        PolicyDecision::Rejected(RejectionReason::SpoolRefused { .. })
    ));

    let taken = charges_to(world, next.id);
    assert_eq!(taken.len(), 1);
    assert_close(
        spool_now(world, next.id).remaining_g,
        1_000.0 - grams(&next, TOTAL_MM - STOPPED_MM),
    );
    assert!(pending(world).is_empty());
    assert_close(change.accounted_mm(), TOTAL_MM);
}

/// A second signal at the same position adds no length to anything: what the
/// tool fed is still charged and held exactly once.
#[test]
fn a_second_signal_at_the_same_position_adds_nothing() {
    let change = signalled(true, false);
    let world = &change.world;
    paused_at(world, STOPPED_AT);
    world
        .handle(obico_filament_change(world))
        .expect("the second alert is handled");
    assert_eq!(every::<FilamentChangeSignalledPayload>(world).len(), 2);
    assert!(every::<ChargePendedPayload>(world).is_empty());
    assert_eq!(change.charges().len(), 1);
    change.completes();
    assert_eq!(every::<ChargePendedPayload>(world).len(), 1);
    assert_close(change.accounted_mm(), TOTAL_MM);
}

/// A supervisor built after the remainder was held still lists it, and a load
/// through it takes it once.
#[test]
fn a_restarted_supervisor_lists_what_was_held_and_a_load_takes_it_once() {
    let change = signalled(true, false);
    let world = &change.world;
    change.completes();
    let restarted: Arc<Supervisor> = world.restarted();
    let held = block_on(restarted.pending_charges(Some(0))).expect("read");
    assert_eq!(held.len(), 1);
    let next = register(world, "After Restart", 1_000.0);
    let loaded = command_on(&restarted, load_command(next.id, 0));
    assert_eq!(loaded.decision, PolicyDecision::Accepted);
    assert_eq!(charges_to(world, next.id).len(), 1);
    assert!(
        block_on(restarted.pending_charges(None))
            .expect("read")
            .is_empty()
    );
    assert_close(change.accounted_mm(), TOTAL_MM);
    let _: SpoolId = next.id;
}

/// Unloading the spool an unconfirmed change left takes nothing: what the tool
/// fed after the signal is still held for the next spool loaded, which takes
/// it.
#[test]
fn unloading_the_spool_after_a_change_takes_nothing() {
    let change = signalled(true, false);
    let world = &change.world;
    running_at(world, 600_000);
    world
        .printer
        .extrudes(FILE, 600_000, vec![on_tool(0, 3_000.0)]);
    let unloaded = command(
        world,
        SpoolCommand::UnloadSpool {
            spool_id: change.spool.id,
            reason: "took the old spool off".to_owned(),
            actor: printobserver_core::Actor::Operator,
        },
    );
    assert_eq!(unloaded.decision, PolicyDecision::Accepted);
    assert_eq!(change.charges().len(), 1, "the unload charged something");
    change.completes();
    assert_eq!(every::<ChargePendedPayload>(world).len(), 1);
    assert_close(pending(world)[0].length_mm, TOTAL_MM - STOPPED_MM);
    let next = register(world, "Fed It", 1_000.0);
    assert_eq!(load(world, next.id, 0).decision, PolicyDecision::Accepted);
    assert_eq!(charges_to(world, next.id).len(), 1);
    assert_close(change.accounted_mm(), TOTAL_MM);
}

/// A change signalled with no spool loaded is written down naming none, and
/// charges and marks nothing.
#[test]
fn a_change_with_no_spool_loaded_charges_nothing() {
    let world = spool_world_with(|_| {});
    world.clock.advance(80_000_000);
    let print_id = start(&world);
    paused_at(&world, STOPPED_AT);
    world
        .handle(obico_filament_change(&world))
        .expect("the alert is handled");
    let signals = every::<FilamentChangeSignalledPayload>(&world);
    assert_eq!(signals.len(), 1);
    assert_eq!(signals[0].tool, None);
    assert_eq!(signals[0].spool_id, None);
    assert!(
        signals[0]
            .position
            .as_ref()
            .is_some_and(|signal| signal.read_at_signal)
    );
    assert!(every_of::<FilamentConsumedPayload>(&world, print_id).is_empty());
}

/// A signal whose ledger write was refused — or committed with its answer
/// lost — and whose alert then arrives again charges the spool to the stop
/// point once and marks it once.
#[test]
fn a_signal_replayed_after_its_write_failed_charges_once() {
    for lost in [false, true] {
        let world = spool_world_with(|_| {});
        world.clock.advance(80_000_000);
        let spool = loaded_spool(&world, "Replayed Signal", 1_000.0);
        let print_id = start(&world);
        world
            .printer
            .extrudes(FILE, STOPPED_AT, vec![on_tool(0, STOPPED_MM)]);
        paused_at(&world, STOPPED_AT);
        let error = StoreError::Database {
            detail: "the connection dropped".to_owned(),
        };
        if lost {
            world
                .store
                .loses_the_answer_to(StoreMethod::ApplySpoolChanges, error);
        } else {
            world.store.fails(StoreMethod::ApplySpoolChanges, error);
        }
        world
            .handle(obico_filament_change(&world))
            .expect_err("the ledger write failed");
        world.store.heals();
        world
            .handle(obico_filament_change(&world))
            .expect("the alert arriving again is handled");

        let charges = every_of::<FilamentConsumedPayload>(&world, print_id);
        assert_eq!(charges.len(), 1, "lost={lost}");
        assert_close(charges[0].length_mm, STOPPED_MM);
        assert!(
            every::<ChargePendedPayload>(&world).is_empty(),
            "lost={lost}"
        );
        assert_eq!(
            reasons(&spool_now(&world, spool.id)),
            vec![UnverifiedReason::FilamentChangeUnconfirmed { print_id }]
        );
        assert_ledger_explains(&world, spool.id);
    }
}

/// A tool emptied after an unconfirmed change and loaded again during the
/// print charges the spool loaded what the tool fed since the signal, rather
/// than recording it against no spool.
#[test]
fn a_load_onto_a_tool_emptied_after_a_change_takes_what_was_fed_since() {
    let change = signalled(true, false);
    let world = &change.world;
    let unloaded = command(
        world,
        SpoolCommand::UnloadSpool {
            spool_id: change.spool.id,
            reason: "took the old spool off".to_owned(),
            actor: printobserver_core::Actor::Operator,
        },
    );
    assert_eq!(unloaded.decision, PolicyDecision::Accepted);
    let next = register(world, "Onto The Empty Tool", 1_000.0);
    running_at(world, 600_000);
    world
        .printer
        .extrudes(FILE, 600_000, vec![on_tool(0, 3_000.0)]);
    assert_eq!(load(world, next.id, 0).decision, PolicyDecision::Accepted);
    change.completes();

    let to_next = charges_to(world, next.id);
    assert_eq!(to_next.len(), 2);
    assert_close(to_next[0].length_mm, 3_000.0 - STOPPED_MM);
    assert!(
        change
            .charges()
            .iter()
            .all(|charge| charge.charged.is_some()),
        "a charge went to no spool"
    );
    assert_close(change.accounted_mm(), TOTAL_MM);
}
