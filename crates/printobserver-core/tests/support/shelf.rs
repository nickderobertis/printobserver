//! The spools, the file and the print the filament journeys drive, and the
//! reads they make of the timeline afterwards.
//!
//! Everything here drives the supervisor the way a caller does — spool commands
//! through [`Supervisor::request_spool_command`], starts and cancels through
//! [`Supervisor::request_action`], the printer's job read back through the
//! listing — and reads the outcome back through the timeline and the spool
//! records, never through the store's own internals.

use std::sync::Arc;

use printobserver_core::kinds::FilamentConsumedPayload;
use printobserver_core::store::TimelineQuery;
use printobserver_core::{
    ActionKind, ActionOutcome, Actor, ActorClass, CoreConfig, PolicyDecision, PrintAction,
    SafetyEnvelope, SpoolCommand, SpoolCommandOutcome, SpoolId, SpoolRecord, Supervisor, block_on,
    grams_for_length,
};
use printobserver_printer_api::{FileFacts, FilePrint, PrinterState, ToolFilament};
use printobserver_types::serde::de::DeserializeOwned;
use printobserver_types::{EventPayload, EventRecord, FileName, PrintId, Timestamp};

use crate::world::{World, manifest, permissive_envelope};

/// The file every filament journey prints.
pub const FILE: &str = "benchy.gcode";

/// Its size, in bytes.
pub const SIZE: i64 = 1_000_000;

/// Its analysed length on tool 0, in millimetres.
pub const TOTAL_MM: f64 = 5_000.0;

/// Every spool command, as the action kind a safety envelope grants.
pub const SPOOL_KINDS: [ActionKind; 5] = [
    ActionKind::RegisterSpool,
    ActionKind::LoadSpool,
    ActionKind::UnloadSpool,
    ActionKind::AdjustSpool,
    ActionKind::RetireSpool,
];

/// The permissive envelope, granting every class every spool command too.
#[must_use]
pub fn spool_envelope() -> SafetyEnvelope {
    let mut envelope = permissive_envelope();
    for kinds in envelope.actions.values_mut() {
        kinds.extend(SPOOL_KINDS);
    }
    envelope
}

/// The spool envelope with one class granted nothing of the spool ledger.
#[must_use]
pub fn envelope_without_spools_for(class: ActorClass) -> SafetyEnvelope {
    let mut envelope = spool_envelope();
    if let Some(kinds) = envelope.actions.get_mut(&class) {
        kinds.retain(|kind| !SPOOL_KINDS.contains(kind));
    }
    envelope
}

/// A world under the spool envelope, holding the file with its analysis.
#[must_use]
pub fn spool_world() -> World {
    spool_world_with(|_| {})
}

/// A world under the spool envelope and a configuration one journey edits,
/// holding the file with its analysis.
#[must_use]
pub fn spool_world_with(edit: impl FnOnce(&mut CoreConfig)) -> World {
    let world = World::with_config(spool_envelope(), edit);
    world.printer.holds_file(facts(Some(TOTAL_MM)));
    world
}

/// The file's facts, with this analysed length on tool 0 or none.
#[must_use]
pub fn facts(length_mm: Option<f64>) -> FileFacts {
    FileFacts {
        file_name: FILE.to_owned(),
        origin: "local".to_owned(),
        size_bytes: Some(SIZE),
        filament: length_mm.map(|length_mm| {
            vec![ToolFilament {
                tool: 0,
                length_mm,
                volume_cm3: None,
            }]
        }),
        successes: 0,
        failures: 0,
        last_print: None,
    }
}

/// One tool's filament, as the printer reports it.
#[must_use]
pub const fn on_tool(tool: i64, length_mm: f64) -> ToolFilament {
    ToolFilament {
        tool,
        length_mm,
        volume_cm3: None,
    }
}

/// Carry out one spool command through the real path.
///
/// # Panics
///
/// Panics when the store refuses it outright.
pub fn command(world: &World, command: SpoolCommand) -> SpoolCommandOutcome {
    command_on(&world.core, command)
}

/// Carry out one spool command through one supervisor.
///
/// # Panics
///
/// Panics when the store refuses it outright.
pub fn command_on(core: &Arc<Supervisor>, command: SpoolCommand) -> SpoolCommandOutcome {
    block_on(core.request_spool_command(command)).expect("the command is written down")
}

/// Register one PLA spool as the operator, with a 250 g tare.
///
/// # Panics
///
/// Panics when the registration is refused.
pub fn register(world: &World, name: &str, initial_g: f64) -> SpoolRecord {
    let outcome = command(
        world,
        SpoolCommand::RegisterSpool {
            name: name.to_owned(),
            material: "PLA".to_owned(),
            color: None,
            diameter_mm: None,
            density_g_cm3: None,
            initial_g,
            remaining_g: None,
            tare_g: Some(250.0),
            reason: "a new spool on the shelf".to_owned(),
            actor: Actor::Operator,
        },
    );
    assert_eq!(outcome.decision, PolicyDecision::Accepted);
    outcome.spool.expect("a registration answers its spool")
}

/// The operator's load of one spool onto one tool.
#[must_use]
pub fn load_command(spool_id: SpoolId, tool: i64) -> SpoolCommand {
    SpoolCommand::LoadSpool {
        spool_id,
        tool,
        reason: "the spool went on the printer".to_owned(),
        actor: Actor::Operator,
    }
}

/// Load one spool onto one tool as the operator.
pub fn load(world: &World, spool_id: SpoolId, tool: i64) -> SpoolCommandOutcome {
    command(world, load_command(spool_id, tool))
}

/// The operator's correction of one spool to a figure.
#[must_use]
pub fn adjust_command(spool_id: SpoolId, remaining_g: f64) -> SpoolCommand {
    SpoolCommand::AdjustSpool {
        spool_id,
        remaining_g: Some(remaining_g),
        gross_g: None,
        reason: "weighed on the scale".to_owned(),
        actor: Actor::Operator,
    }
}

/// Register a spool and load it onto tool 0.
#[must_use]
pub fn loaded_spool(world: &World, name: &str, initial_g: f64) -> SpoolRecord {
    let spool = register(world, name, initial_g);
    assert_eq!(load(world, spool.id, 0).decision, PolicyDecision::Accepted);
    spool_now(world, spool.id)
}

/// One spool as it stands now.
///
/// # Panics
///
/// Panics when there is no such spool.
#[must_use]
pub fn spool_now(world: &World, spool_id: SpoolId) -> SpoolRecord {
    block_on(world.core.spool(spool_id))
        .expect("the spool is read")
        .expect("the spool exists")
}

/// What a length weighs on one spool.
#[must_use]
pub fn grams(spool: &SpoolRecord, length_mm: f64) -> f64 {
    grams_for_length(length_mm, spool.diameter_mm, spool.density_g_cm3)
}

/// The whole printer timeline, oldest first.
///
/// # Panics
///
/// Panics when the timeline cannot be read.
#[must_use]
pub fn timeline(world: &World) -> Vec<EventRecord> {
    let mut events = block_on(world.core.timeline(TimelineQuery {
        kinds: Vec::new(),
        since: None,
        until: None,
        spool_id: None,
        before: None,
        limit: Some(printobserver_core::MAX_HISTORY_LIMIT),
    }))
    .expect("the timeline is read");
    events.reverse();
    events
}

/// Every timeline event of one kind, oldest first, read as its payload.
///
/// # Panics
///
/// Panics when an event of the kind will not read as it.
#[must_use]
pub fn every<P: EventPayload + DeserializeOwned>(world: &World) -> Vec<P> {
    timeline(world)
        .into_iter()
        .filter_map(|event| event.payload_as::<P>())
        .map(|read| read.expect("a timeline payload reads as its own kind"))
        .collect()
}

/// Every timeline event of one kind about one print, oldest first.
#[must_use]
pub fn every_of<P: EventPayload + DeserializeOwned>(world: &World, print_id: PrintId) -> Vec<P> {
    timeline(world)
        .into_iter()
        .filter(|event| event.print_id == Some(print_id))
        .filter_map(|event| event.payload_as::<P>())
        .map(|read| read.expect("a timeline payload reads as its own kind"))
        .collect()
}

/// Every charge made against one spool, oldest first.
#[must_use]
pub fn charges_to(world: &World, spool_id: SpoolId) -> Vec<FilamentConsumedPayload> {
    every::<FilamentConsumedPayload>(world)
        .into_iter()
        .filter(|charge| charge.spool_id == Some(spool_id))
        .collect()
}

/// The start of the file, by one actor, overriding the check or not.
#[must_use]
pub fn start_action(actor: Actor, override_filament_check: bool) -> PrintAction {
    PrintAction::StartPrint {
        file_name: FileName::new(FILE).expect("a file name"),
        manifest: manifest(&[]),
        override_filament_check,
        reason: "the next print in the queue".to_owned(),
        actor,
    }
}

/// Ask an idle printer to start the file, through the real action path.
///
/// # Panics
///
/// Panics when the request could not be recorded.
pub fn request_start(world: &World, actor: Actor, override_filament_check: bool) -> ActionOutcome {
    world.printer.reports_state(PrinterState::Operational);
    world.printer.reports_job(None, PrinterState::Operational);
    world.printer.reports_position(None, None);
    world.printer.reports_completion(None);
    world
        .request(PrintId::new(), start_action(actor, override_filament_check))
        .expect("the start is recorded")
}

/// Start the file as the operator, and have the printer report it running.
///
/// # Panics
///
/// Panics when the start was not taken.
pub fn start(world: &World) -> PrintId {
    let outcome = request_start(world, Actor::Operator, false);
    assert!(
        outcome.executed == Some(Ok(())),
        "the start was taken: {:?}",
        outcome.record.decision
    );
    running_at(world, 0);
    outcome.record.print_id
}

/// Have the printer report the job running at a position, and read it.
pub fn running_at(world: &World, position_bytes: i64) {
    running_at_on(world, &world.core, position_bytes);
}

/// Have the printer report the job running at a position, and read it
/// through one supervisor.
///
/// # Panics
///
/// Panics when the listing fails.
pub fn running_at_on(world: &World, core: &Arc<Supervisor>, position_bytes: i64) {
    world.clock.advance(60);
    world.printer.reports_state(PrinterState::Printing);
    world
        .printer
        .reports_job(Some(FILE), PrinterState::Printing);
    world
        .printer
        .reports_position(Some(position_bytes), Some(SIZE));
    block_on(core.list_and_adopt_prints()).expect("the listing reads");
}

/// Have the printer report the job paused at a position, without reading it.
pub fn paused_at(world: &World, position_bytes: i64) {
    world.clock.advance(60);
    world.printer.reports_state(PrinterState::Paused);
    world.printer.reports_job(Some(FILE), PrinterState::Paused);
    world
        .printer
        .reports_position(Some(position_bytes), Some(SIZE));
}

/// Have the printer report the job over in a state, with the position
/// `OctoPrint` clears as a job ends, and read it.
pub fn ends(world: &World, state: PrinterState) {
    ends_on(world, &world.core, state);
}

/// Have the printer report the job over in a state, and read it through one
/// supervisor.
///
/// # Panics
///
/// Panics when the listing fails.
pub fn ends_on(world: &World, core: &Arc<Supervisor>, state: PrinterState) {
    world.clock.advance(60);
    world.printer.reports_state(state.clone());
    world.printer.reports_job(Some(FILE), state);
    world.printer.reports_position(None, Some(SIZE));
    block_on(core.list_and_adopt_prints()).expect("the listing reads");
}

/// Record on the file's tally that one more print of it completed, now.
pub fn tally_success(world: &World, at: Timestamp) {
    let mut held = world.printer.file(FILE);
    held.successes += 1;
    held.last_print = Some(FilePrint { at, success: true });
    world.printer.holds_file(held);
}

/// Record on the file's tally that one more print of it failed, now.
pub fn tally_failure(world: &World, at: Timestamp) {
    let mut held = world.printer.file(FILE);
    held.failures += 1;
    held.last_print = Some(FilePrint { at, success: false });
    world.printer.holds_file(held);
}

/// Assert two figures agree to within a rounding error.
#[track_caller]
pub fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-6,
        "expected {expected}, found {actual}"
    );
}

/// Assert a spool's figure is its initial figure less every charge against it
/// plus every correction of it, read off its timeline, and that the timeline's
/// before and after figures chain from one event to the next.
#[track_caller]
pub fn assert_ledger_explains(world: &World, spool_id: SpoolId) {
    use printobserver_core::kinds::{SpoolAdjustedPayload, SpoolRegisteredPayload};
    let spool = spool_now(world, spool_id);
    let mut figure = None;
    for event in timeline(world) {
        if let Some(Ok(registered)) = event.payload_as::<SpoolRegisteredPayload>()
            && registered.spool.id == spool_id
        {
            figure = Some(registered.spool.remaining_g);
        }
        if let Some(Ok(charge)) = event.payload_as::<FilamentConsumedPayload>()
            && charge.spool_id == Some(spool_id)
        {
            let before = charge
                .remaining_before_g
                .expect("a charge names its figure");
            assert_close(before, figure.expect("registered first"));
            let after = charge.remaining_after_g.expect("a charge names its figure");
            assert_close(after, before - charge.grams.expect("a charge weighs"));
            figure = Some(after);
        }
        if let Some(Ok(adjusted)) = event.payload_as::<SpoolAdjustedPayload>()
            && adjusted.spool_id == spool_id
        {
            assert_close(
                adjusted.remaining_before_g,
                figure.expect("registered first"),
            );
            figure = Some(adjusted.remaining_after_g);
        }
    }
    assert_close(
        spool.remaining_g,
        figure.expect("the spool is in the timeline"),
    );
}
