//! The records the filament ledger's journeys drive a store with.
//!
//! Each event is built from the payload type the supervision domain declares
//! for its kind, so a journey writes what the domain will write rather than a
//! shape of its own.

use std::sync::Arc;

use crate::block_on::block_on;
use crate::fixture::{Store, instant};
use printobserver_core::kinds::{
    ChargePendedPayload, FilamentConsumedPayload, PrintEndedPayload, SpoolAdjustedPayload,
    SpoolLoadedPayload, SpoolRetiredPayload, SpoolUnloadedPayload, SpoolUnverifiedPayload,
    UnaccountedPrintPayload, operator_source, system_source,
};
use printobserver_core::store::{
    Charge, ChargeRecord, EventDraft, FileTally, NewSpool, Pend, SpoolChange, SpoolMutation,
    TimelineQuery,
};
use printobserver_core::{
    Actor, ChargeKey, ChargeMethod, PendingCharge, PrintOutcome, SpoolId, SpoolRecord, SpoolTrust,
};
use printobserver_printer_api::PrinterState;
use printobserver_types::{EventBody, EventPayload, EventRecord, PrintId, Timestamp};

/// The instant `minute` minutes after noon on the day every journey is set on.
pub fn at(minute: u32) -> Timestamp {
    instant(&format!(
        "2026-03-01T{:02}:{:02}:00Z",
        12 + minute / 60,
        minute % 60
    ))
}

/// One event of a declared kind, from the supervisor.
pub fn event_of<P: EventPayload>(
    print_id: Option<PrintId>,
    received_at: Timestamp,
    payload: &P,
) -> EventDraft {
    EventDraft {
        print_id,
        source: system_source(),
        received_at,
        body: EventBody::of(payload).expect("a declared payload serializes"),
        raw: None,
    }
}

/// A spool as an operator registers it.
pub fn new_spool(
    name: &str,
    loaded_on_tool: Option<i64>,
    remaining_g: f64,
    registered_at: Timestamp,
) -> NewSpool {
    NewSpool {
        name: name.to_owned(),
        material: "PLA".to_owned(),
        color: Some("black".to_owned()),
        diameter_mm: 1.75,
        density_g_cm3: 1.24,
        initial_g: 1_000.0,
        remaining_g,
        tare_g: Some(250.0),
        loaded_on_tool,
        trust: SpoolTrust::Verified,
        registered_at,
    }
}

/// Register one spool.
///
/// # Panics
///
/// Panics when the store refuses it.
pub fn register(
    port: &Arc<dyn Store>,
    name: &str,
    loaded_on_tool: Option<i64>,
    remaining_g: f64,
    registered_at: Timestamp,
) -> SpoolRecord {
    block_on(port.register_spool(
        new_spool(name, loaded_on_tool, remaining_g, registered_at),
        operator_source(),
        Actor::Operator,
        "a new spool on the shelf".to_owned(),
    ))
    .expect("the spool registers")
    .0
}

/// The reason every spool command in these journeys gives.
fn reason() -> String {
    "the operator said so".to_owned()
}

/// One change to one spool, with the event the domain writes for it.
pub fn change(spool_id: SpoolId, change: SpoolChange, at: Timestamp) -> SpoolMutation {
    let event = match &change {
        SpoolChange::Load { tool } => event_of(
            None,
            at,
            &SpoolLoadedPayload {
                spool_id,
                tool: *tool,
                replaced: None,
                took_pending: Vec::new(),
                cleared_marks: Vec::new(),
                actor: Actor::Operator,
                reason: reason(),
            },
        ),
        SpoolChange::Unload => event_of(
            None,
            at,
            &SpoolUnloadedPayload {
                spool_id,
                tool: 0,
                actor: Actor::Operator,
                reason: reason(),
            },
        ),
        SpoolChange::SetRemaining { remaining_g } => event_of(
            None,
            at,
            &SpoolAdjustedPayload {
                spool_id,
                remaining_before_g: 0.0,
                remaining_after_g: *remaining_g,
                gross_g: None,
                took_pending: Vec::new(),
                cleared_marks: Vec::new(),
                actor: Actor::Operator,
                reason: reason(),
            },
        ),
        SpoolChange::MarkUnverified { reason } => event_of(
            None,
            at,
            &SpoolUnverifiedPayload {
                spool_id,
                reason: reason.clone(),
            },
        ),
        SpoolChange::ClearMarks { .. } => event_of(
            None,
            at,
            &SpoolAdjustedPayload {
                spool_id,
                remaining_before_g: 0.0,
                remaining_after_g: 0.0,
                gross_g: None,
                took_pending: Vec::new(),
                cleared_marks: Vec::new(),
                actor: Actor::Operator,
                reason: reason(),
            },
        ),
        SpoolChange::Retire => event_of(
            None,
            at,
            &SpoolRetiredPayload {
                spool_id,
                remaining_g: 0.0,
                actor: Actor::Operator,
                reason: reason(),
            },
        ),
    };
    SpoolMutation {
        spool_id,
        change,
        at,
        event,
    }
}

/// The key of one segment of a watched print on one tool.
pub const fn print_key(print_id: PrintId, tool: i64, segment: u32) -> ChargeKey {
    ChargeKey::Print {
        print_id,
        tool,
        segment,
    }
}

/// The key of one print of a file this system never saw.
pub fn unobserved_key(file_name: &str, generation: u32, print_number: i64) -> ChargeKey {
    ChargeKey::Unobserved {
        origin: "local".to_owned(),
        file_name: file_name.to_owned(),
        generation,
        print_number,
        tool: 0,
    }
}

/// The tool a key names.
const fn tool_of(key: &ChargeKey) -> i64 {
    match key {
        ChargeKey::Print { tool, .. } | ChargeKey::Unobserved { tool, .. } => *tool,
    }
}

/// One debit, with its `filament_consumed` event.
pub fn charge(
    key: ChargeKey,
    print_id: Option<PrintId>,
    spool_id: Option<SpoolId>,
    grams: Option<f64>,
    at: Timestamp,
) -> Charge {
    let tool = tool_of(&key);
    let event = event_of(
        print_id,
        at,
        &FilamentConsumedPayload {
            charge_key: key.clone(),
            tool,
            spool_id,
            method: ChargeMethod::GcodeExtrusion,
            length_mm: 1_000.0,
            grams,
            diameter_mm: None,
            density_g_cm3: None,
            analysed_length_mm: None,
            position: None,
            remaining_before_g: None,
            remaining_after_g: None,
            note: None,
        },
    );
    Charge {
        key,
        tool,
        print_id,
        spool_id,
        length_mm: 1_000.0,
        grams,
        at,
        event,
    }
}

/// One charge held for a spool not yet named, with its `charge_pended` event.
pub fn pend(key: ChargeKey, print_id: PrintId, at: Timestamp) -> Pend {
    let pending = PendingCharge {
        tool: tool_of(&key),
        charge_key: key,
        print_id,
        length_mm: 400.0,
        method: ChargeMethod::GcodeExtrusion,
        analysed_length_mm: None,
        position: None,
        note: None,
        pended_at: at,
    };
    let event = event_of(
        Some(print_id),
        at,
        &ChargePendedPayload {
            pending: pending.clone(),
        },
    );
    Pend { pending, event }
}

/// A print's `print_ended` event.
pub fn ended(print_id: PrintId, outcome: PrintOutcome, at: Timestamp) -> EventDraft {
    event_of(
        Some(print_id),
        at,
        &PrintEndedPayload {
            outcome,
            state: PrinterState::Operational,
            reason: "finished".to_owned(),
            last_position: None,
        },
    )
}

/// An `unaccounted_print` event for a print of a file this system never saw.
pub fn unaccounted(file_name: &str, at: Timestamp) -> EventDraft {
    event_of(
        None,
        at,
        &UnaccountedPrintPayload {
            file_name: file_name.to_owned(),
            printed_at: Some(at),
            success: true,
            charged: true,
        },
    )
}

/// One file's tally.
pub fn tally(file_name: &str, generation: u32, successes: i64, at: Timestamp) -> FileTally {
    FileTally {
        file_name: file_name.to_owned(),
        origin: "local".to_owned(),
        generation,
        successes,
        failures: 0,
        observed_at: at,
    }
}

/// A timeline query with no filter, under a limit.
pub fn whole_timeline(limit: Option<u32>) -> TimelineQuery {
    TimelineQuery {
        kinds: Vec::new(),
        since: None,
        until: None,
        spool_id: None,
        before: None,
        limit,
    }
}

/// The newest thousand timeline events the store holds — every one, for the
/// journeys here, none of which writes so many — newest first.
///
/// # Panics
///
/// Panics when the store refuses the read.
pub fn newest_timeline_events(port: &Arc<dyn Store>) -> Vec<EventRecord> {
    block_on(port.timeline(whole_timeline(Some(1_000)))).expect("the timeline reads")
}

/// The kinds of some events, in order.
pub fn kinds(events: &[EventRecord]) -> Vec<String> {
    events
        .iter()
        .map(|event| event.kind().as_str().to_owned())
        .collect()
}

/// Everything the ledger holds that a write could change, for a journey
/// asserting a refused write changed none of it.
#[derive(Debug, Clone, PartialEq)]
pub struct LedgerState {
    /// Every spool, retired ones included.
    pub spools: Vec<SpoolRecord>,
    /// The newest thousand timeline events.
    pub events: Vec<EventRecord>,
    /// Every charge against each named print.
    pub charges: Vec<Vec<ChargeRecord>>,
    /// Every pending charge.
    pub pending: Vec<PendingCharge>,
    /// Every file's tally.
    pub tallies: Vec<FileTally>,
    /// Each named print's settlement.
    pub settlements: Vec<Option<(Timestamp, PrintOutcome)>>,
}

/// What the ledger holds now, with the charges and settlements of these prints.
///
/// # Panics
///
/// Panics when the store refuses a read.
pub fn ledger_state(port: &Arc<dyn Store>, prints: &[PrintId]) -> LedgerState {
    LedgerState {
        spools: block_on(port.spools(true)).expect("the spools read"),
        events: newest_timeline_events(port),
        charges: prints
            .iter()
            .map(|print_id| block_on(port.charges(*print_id)).expect("the charges read"))
            .collect(),
        pending: block_on(port.pending_charges(None)).expect("the pending charges read"),
        tallies: block_on(port.file_tallies()).expect("the tallies read"),
        settlements: prints
            .iter()
            .map(|print_id| {
                block_on(port.print_settlement(*print_id)).expect("the settlement reads")
            })
            .collect(),
    }
}
