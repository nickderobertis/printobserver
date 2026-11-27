//! A print stopped at a colour change its own G-code plans is left alone
//! until it carries on, then checked once at once and charged colour by
//! colour.
//!
//! Every journey here prints the committed two-change file, in `PrusaSlicer`
//! 2.9.6's syntax, through the real supervisor over its port fakes, on the fake
//! clock, so a forty-minute wait takes no time. The printer watch and the
//! periodic observation are driven exactly as the server's drivers drive them
//! — [`Supervisor::watch_printer`] and [`Supervisor::observe_active_prints`] —
//! and every turn they claim is run. `Obico`'s `filament_change` notification
//! arrives as the alert the Obico adapter normalizes the posted body to, read
//! from the fixture that adapter's own ingress test pins its output to.
//!
//! [`Supervisor::watch_printer`]: printobserver_core::Supervisor::watch_printer
//! [`Supervisor::observe_active_prints`]: printobserver_core::Supervisor::observe_active_prints

use std::sync::{Arc, Mutex, PoisonError};

use printobserver_core::gcode_index::index_of;
use printobserver_core::kinds::{
    FilamentChangePlannedReachedPayload, FilamentChangePlannedResumedPayload,
    FilamentChangeSignalledPayload, FilamentConsumedPayload, PlannedChangeTrigger,
};
use printobserver_core::{ActionOutcome, ChargeKey, Preflight, PreflightCheckedPayload};
use printobserver_core::{
    Actor, CheckImageRequest, CheckImages, Clock as _, GcodeIndex, HeadPosition,
    PLANNED_CHANGE_REACH_BYTES, PendingTurn, PeriodicObservationPayload, PlannedChange,
    PlannedSegment, PolicyDecision, PositionProbe, PreflightVerdict, PrintAction, RejectionReason,
    SegmentSpool, ShownImage, SpoolRecord, block_on,
};
use printobserver_printer_api::{BoxFuture, PrinterError, PrinterState};
use printobserver_types::{EventKind, EventPayload, EventRecord, PrintId};
use printobserver_vision_api::NormalizedAlert;

use printobserver_core::store::StoreError;

use crate::fakes::{PrinterMethod, StoreMethod, printer_snapshot};
use crate::shelf::{
    FILE, SIZE, assert_close, every_of, facts, finishes, grams, load, loaded_spool, on_tool,
    register, request_start, running_at, spool_world, start, start_action,
};
use crate::world::{World, agent_actor};

/// The committed file: six layers, a change to red at layer 3 and one to blue
/// at layer 5.
const TWO_COLOUR: &str = include_str!("../fixtures/planned-changes/two-colour.gcode");

/// The offset of the first byte of the `nth` line reading exactly `line`.
fn line_offset(line: &str, nth: usize) -> i64 {
    let mut at = 0;
    let mut seen = 0;
    for each in TWO_COLOUR.split_inclusive('\n') {
        if each.trim_end() == line {
            if seen == nth {
                return i64::try_from(at).expect("a small offset");
            }
            seen += 1;
        }
        at += each.len();
    }
    panic!("the file has no line {nth} reading {line:?}");
}

/// The offset just past the `nth` `M600` line: where `OctoPrint` stands once
/// it has sent the change and the firmware holds the print there.
fn past_change(nth: usize) -> i64 {
    line_offset("M600", nth) + i64::try_from("M600\n".len()).expect("small")
}

/// A position past the `nth` change and its waiting window: the print has
/// carried on.
fn carried_on_from(nth: usize) -> i64 {
    line_offset("M600", nth) + i64::try_from(PLANNED_CHANGE_REACH_BYTES).expect("small") + 64
}

/// The file's own index, read by the real builder, which every figure here is
/// measured from.
fn the_index() -> GcodeIndex {
    index_of(&[TWO_COLOUR.as_bytes()])
}

/// One segment of the file's plan.
fn segment(index: u32) -> PlannedSegment {
    the_index()
        .planned_segments()
        .into_iter()
        .find(|segment| segment.segment == index)
        .expect("the file plans the segment")
}

/// One change of the file's plan.
fn change(index: u32) -> PlannedChange {
    the_index()
        .planned_change(index)
        .cloned()
        .expect("the file plans the change")
}

/// The filament the file feeds before each byte, as the printer's own reader
/// would answer it: what the index's segments add up to at each change.
fn fed_before(change_index: usize) -> f64 {
    the_index().planned_segments()[..=change_index]
        .iter()
        .map(|segment| segment.filament_mm)
        .sum()
}

/// Where the head is: the watch asks only after a job has ended, and these
/// prints never end that way.
struct NoHead;

impl PositionProbe for NoHead {
    fn read_position(&self) -> BoxFuture<'_, Option<HeadPosition>> {
        Box::pin(async { None })
    }
}

/// What the image maker was asked, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Asked {
    /// The growth measurement was told to start again for a print.
    RestartGrowth(PrintId),
    /// A check's image was made, for this event's frame.
    CheckImage,
}

/// Records what it is asked and shows each check its stored frame.
#[derive(Default)]
struct RecordingImages {
    /// Everything asked, in order.
    asked: Mutex<Vec<Asked>>,
}

impl RecordingImages {
    /// Everything asked so far.
    fn asked(&self) -> Vec<Asked> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl CheckImages for RecordingImages {
    fn check_image(
        &self,
        _print_id: Option<PrintId>,
        frame: &std::path::Path,
    ) -> Result<std::path::PathBuf, String> {
        Ok(frame.to_path_buf())
    }

    fn shown_image(&self, request: &CheckImageRequest<'_>) -> Result<ShownImage, String> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Asked::CheckImage);
        Ok(ShownImage {
            path: request.frame.to_path_buf(),
            note: None,
            composite: false,
        })
    }

    fn restart_growth(&self, print_id: PrintId) {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Asked::RestartGrowth(print_id));
    }
}

/// A world with a camera, holding the two-colour file with its analysis, the
/// white spool loaded on tool 0, and every class granted every action.
fn colour_world() -> (World, SpoolRecord, Arc<RecordingImages>) {
    let world = World::with_config(crate::shelf::spool_envelope(), |config| {
        config.camera_snapshot_url = Some(
            printobserver_vision_api::WebAddress::new(crate::world::CAMERA_URL)
                .expect("the camera's address is one"),
        );
    });
    world.printer.holds_file(facts(Some(fed_before(2))));
    world.printer.holds_gcode(FILE, TWO_COLOUR);
    let images = Arc::new(RecordingImages::default());
    assert!(world.core.install_check_images(images.clone()));
    // The alert names the print Obico saw start, so the clock stands after it.
    world.clock.advance(80_000_000);
    let white = loaded_spool(&world, "White", 1_000.0);
    (world, white, images)
}

/// Start the file and print up to its first layer, with the periodic check
/// there run — which is what reads the file into the print's index.
fn printing(world: &World) -> PrintId {
    let print_id = start(world);
    running_at(world, line_offset("M73 C1", 0));
    let turns = observe(world);
    assert_eq!(turns.len(), 1, "the print was not checked while it printed");
    run(world, turns);
    print_id
}

/// One round of the printer watch, as the server's driver runs it.
fn watch(world: &World) -> Vec<PendingTurn> {
    block_on(world.core.watch_printer(&NoHead))
        .expect("the watch records what it found")
        .turns
}

/// One round of the periodic observation, as the server's driver runs it.
fn observe(world: &World) -> Vec<PendingTurn> {
    let observed = block_on(world.core.observe_active_prints()).expect("the prints are read");
    assert!(observed.refused.is_empty(), "{:?}", observed.refused);
    observed.turns
}

/// Run every turn claimed.
fn run(world: &World, turns: Vec<PendingTurn>) {
    for turn in turns {
        block_on(world.core.run_supervision(turn)).expect("the turn is supervised");
    }
}

/// The printer reports the job printing at a position: `OctoPrint` stays
/// printing through the firmware's own wait at an `M600`.
fn printing_at(world: &World, position: i64) {
    world.printer.reports_state(PrinterState::Printing);
    world
        .printer
        .reports_job(Some(FILE), PrinterState::Printing);
    world.printer.reports_position(Some(position), Some(SIZE));
}

/// `Obico`'s `filament_change` notification, as its adapter normalizes it,
/// received now.
fn obico_filament_change(world: &World) -> NormalizedAlert {
    let mut alert: NormalizedAlert = printobserver_types::serde_json::from_str(include_str!(
        "../../../printobserver-obico/tests/fixtures/filament-change-alert.json"
    ))
    .expect("the committed alert reads");
    alert.received_at = world.clock.now();
    alert
}

/// Every event of one kind in a print's history, oldest first.
fn of_print<P: EventPayload + printobserver_types::serde::de::DeserializeOwned>(
    world: &World,
    print_id: PrintId,
) -> Vec<(EventRecord, P)> {
    let mut events: Vec<(EventRecord, P)> = world
        .store
        .events_of(print_id)
        .into_iter()
        .filter_map(|event| {
            let payload = event.payload_as::<P>()?.ok()?;
            Some((event, payload))
        })
        .collect();
    events.sort_by_key(|(event, _)| event.received_at);
    events
}

/// The one filament check a start's outcome recorded.
fn checked_for(world: &World, outcome: &ActionOutcome) -> Preflight {
    let checked: Vec<PreflightCheckedPayload> =
        crate::shelf::every::<PreflightCheckedPayload>(world)
            .into_iter()
            .filter(|checked| checked.action_id == Some(outcome.record.id))
            .collect();
    assert_eq!(checked.len(), 1, "every start writes one check naming it");
    checked.into_iter().next().expect("one").preflight
}

/// The seven actions a print waiting at a planned change refuses, as one
/// actor asks for each.
fn refused_while_waiting(actor: &Actor) -> Vec<PrintAction> {
    let reason = || "a journey asks while the printer waits".to_owned();
    vec![
        PrintAction::Pause {
            reason: reason(),
            actor: actor.clone(),
        },
        PrintAction::Resume {
            reason: reason(),
            actor: actor.clone(),
        },
        PrintAction::SetFeedrateFactor {
            factor: 1.0,
            duration_s: None,
            reason: reason(),
            actor: actor.clone(),
        },
        PrintAction::SetFlowrateFactor {
            factor: 1.0,
            duration_s: None,
            reason: reason(),
            actor: actor.clone(),
        },
        PrintAction::SetToolTargetC {
            tool: 0,
            target_c: 215.0,
            duration_s: None,
            reason: reason(),
            actor: actor.clone(),
        },
        PrintAction::SetBedTargetC {
            target_c: 60.0,
            duration_s: None,
            reason: reason(),
            actor: actor.clone(),
        },
        PrintAction::SetFanPercent {
            percent: 100.0,
            duration_s: None,
            reason: reason(),
            actor: actor.clone(),
        },
    ]
}

/// The refusal waiting at the first change answers.
fn waiting_at_the_first_change() -> PolicyDecision {
    let first = change(0);
    PolicyDecision::Rejected(RejectionReason::PlannedChangeWaiting {
        change_index: first.index,
        layer: first.layer,
        z_mm: first.z_mm,
        color: first.color,
    })
}

/// A print the job froze at — `OctoPrint` printing, its position just past the
/// `M600` — is reached by that position; `Obico`'s notifications at 0 and 10
/// minutes are the same change and start nothing; through a forty-minute wait
/// crossing the firmware's heater timeout no check runs; every actor's pause,
/// resume and adjustment is refused naming the change while a cancel and a
/// spool command are not; and once the position moves past the change it is
/// recorded resumed and checked at once, the next scheduled check an interval
/// later and the growth measured only from frames after it.
#[test]
fn a_print_frozen_at_a_planned_change_is_left_alone_until_it_carries_on() {
    let (world, white, images) = colour_world();
    let print_id = printing(&world);
    let checks_before = world.agent.turns().len();
    let observations_before = of_print::<PeriodicObservationPayload>(&world, print_id).len();

    world.printer.extrudes(
        FILE,
        line_offset("M600", 0),
        vec![on_tool(0, fed_before(0))],
    );
    printing_at(&world, past_change(0));
    let started = watch(&world);
    assert!(started.is_empty(), "reaching the change claimed a turn");

    let reached = of_print::<FilamentChangePlannedReachedPayload>(&world, print_id);
    assert_eq!(reached.len(), 1, "{reached:?}");
    let (_, reached) = &reached[0];
    assert_eq!(reached.change, change(0));
    assert_eq!(reached.trigger, PlannedChangeTrigger::Position);
    assert_eq!(reached.previous_color.as_deref(), Some("#FFFFFF"));
    assert_eq!(reached.spool_id, Some(white.id));
    assert_eq!(reached.position.bytes, past_change(0));
    assert_close(reached.next_segment_filament_mm, segment(1).filament_mm);

    wait_forty_minutes(&world);
    assert_eq!(
        world.agent.turns().len(),
        checks_before,
        "a turn ran while the printer waited"
    );
    assert_eq!(
        of_print::<PeriodicObservationPayload>(&world, print_id).len(),
        observations_before,
        "an observation was written while the printer waited"
    );
    assert_eq!(
        of_print::<FilamentChangePlannedReachedPayload>(&world, print_id).len(),
        1
    );
    let signalled = every_of::<FilamentChangeSignalledPayload>(&world, print_id);
    assert_eq!(signalled.len(), 1, "{signalled:?}");
    assert_eq!(signalled[0].planned_change, Some(0));

    every_actor_is_refused_but_a_cancel_and_a_spool_command(&world, print_id);

    printing_at(&world, carried_on_from(0));
    let resumed_at = world.clock.elapsed();
    let check = watch(&world);
    assert_eq!(check.len(), 1, "carrying on did not claim one check");
    run(&world, check);
    checked_once_after_the_change(&world, print_id, checks_before, &images);

    let interval = world.config.observation_interval;
    world.clock.advance(60);
    assert!(
        observe(&world).is_empty(),
        "a check ran inside the interval"
    );
    let since_resumed = world.clock.elapsed().saturating_sub(resumed_at);
    assert_eq!(
        world.core.until_next_observation(),
        interval.saturating_sub(since_resumed)
    );
    world.clock.advance(60);
    let next = observe(&world);
    assert_eq!(next.len(), 1, "no check ran an interval after the change's");
    run(&world, next);
    assert_eq!(world.core.until_next_observation(), interval);
    world.journal.assert_no_violations();
}

/// Reads finding the job carried on past the change while its resumption is
/// still being written — the watch's and the scheduled check's land together
/// on a real server — record it once, write no observation and claim no
/// check of their own: the one check is the resumption's.
#[test]
fn reads_past_a_change_at_once_record_its_resumption_once() {
    let (world, _, _) = colour_world();
    let print_id = printing(&world);
    world.printer.extrudes(
        FILE,
        line_offset("M600", 0),
        vec![on_tool(0, fed_before(0))],
    );
    printing_at(&world, past_change(0));
    assert!(
        watch(&world).is_empty(),
        "reaching the change claimed a turn"
    );
    world.clock.advance(120);
    let observations_before = of_print::<PeriodicObservationPayload>(&world, print_id).len();

    printing_at(&world, carried_on_from(0));
    let resumed_kind =
        EventKind::new(FilamentChangePlannedResumedPayload::KIND).expect("the kind is one");
    world.store.holds_appends_of(resumed_kind);
    let (first, watched, observed) = std::thread::scope(|scope| {
        let first = scope.spawn(|| watch(&world));
        world.store.await_a_held_append();
        let watched = watch(&world);
        let observed = observe(&world);
        world.store.lets_appends_through();
        (
            first.join().expect("the first read ends"),
            watched,
            observed,
        )
    });
    assert!(
        watched.is_empty(),
        "the watch's second read claimed a check"
    );
    assert!(observed.is_empty(), "the scheduled check claimed a check");
    assert_eq!(
        of_print::<PeriodicObservationPayload>(&world, print_id).len(),
        observations_before,
        "the scheduled check wrote an observation while the resumption was written"
    );
    let resumed = of_print::<FilamentChangePlannedResumedPayload>(&world, print_id);
    assert_eq!(resumed.len(), 1, "{resumed:?}");
    assert_eq!(resumed[0].1.change, change(0));
    assert_eq!(first.len(), 1, "the first read claimed no check");
    run(&world, first);
    let handed = world.agent.turns();
    assert_eq!(
        handed.last().map(|turn| turn.event.id),
        Some(resumed[0].0.id)
    );
    world.journal.assert_no_violations();
}

/// A resumption the store could not record leaves the print waiting at its
/// change, with no check after it begun: the scheduled check stays held, and
/// the next read once the store heals records the resumption once and claims
/// its check.
#[test]
fn a_resumption_the_store_refused_is_recorded_by_the_next_read() {
    let (world, _, _) = colour_world();
    let print_id = printing(&world);
    world.printer.extrudes(
        FILE,
        line_offset("M600", 0),
        vec![on_tool(0, fed_before(0))],
    );
    printing_at(&world, past_change(0));
    assert!(
        watch(&world).is_empty(),
        "reaching the change claimed a turn"
    );
    world.clock.advance(120);
    let observations_before = of_print::<PeriodicObservationPayload>(&world, print_id).len();

    printing_at(&world, carried_on_from(0));
    world.store.fails(
        StoreMethod::AppendEvent,
        StoreError::Io {
            detail: "the disk is full".to_owned(),
        },
    );
    let refused = block_on(world.core.watch_printer(&NoHead));
    world.store.heals();
    assert!(
        refused
            .as_ref()
            .map_or(true, |watched| watched.turns.is_empty()),
        "a refused resumption claimed a check"
    );
    assert!(
        of_print::<FilamentChangePlannedResumedPayload>(&world, print_id).is_empty(),
        "a refused resumption was recorded"
    );
    assert_eq!(
        block_on(world.core.planned_change_waiting(print_id))
            .expect("the history is read")
            .map(|waiting| waiting.reached.change),
        Some(change(0)),
        "the print no longer waits at the change its resumption was refused at"
    );

    let check = watch(&world);
    assert_eq!(check.len(), 1, "the next read claimed no check");
    let resumed = of_print::<FilamentChangePlannedResumedPayload>(&world, print_id);
    assert_eq!(resumed.len(), 1, "{resumed:?}");
    assert_eq!(resumed[0].1.change, change(0));
    run(&world, check);
    assert!(
        observe(&world).is_empty(),
        "a check ran inside the interval"
    );
    assert_eq!(
        of_print::<PeriodicObservationPayload>(&world, print_id).len(),
        observations_before
    );
    world.journal.assert_no_violations();
}

/// Forty minutes of a printer waiting at a change, read by the watch and the
/// scheduled observation every two: `Obico` notifies at 0 and 10 minutes, and
/// the firmware's idle timeout turns the nozzle off at 30. Nothing is claimed.
fn wait_forty_minutes(world: &World) {
    world
        .handle(obico_filament_change(world))
        .expect("the notification is handled");
    for minute in (2..=40).step_by(2) {
        world.clock.advance(120);
        if minute == 10 {
            world
                .handle(obico_filament_change(world))
                .expect("the repeat is handled");
        }
        if minute == 30 {
            let mut idle = printer_snapshot(PrinterState::Printing);
            idle.tools[0].target_c = Some(printobserver_types::Reported::new(
                0.0,
                printobserver_printer_api::HEATER_TARGET_C_RANGE,
            ));
            world.printer.reports(idle);
        }
        assert!(
            watch(world).is_empty(),
            "the watch claimed a turn at {minute} min"
        );
        assert!(
            observe(world).is_empty(),
            "a scheduled check ran at {minute} min"
        );
    }
}

/// Every one of the seven refused actions, from each of the three actor
/// classes, is refused naming the first change, and reaches no printer; a
/// load of the next colour's spool and a cancel are not refused for it.
fn every_actor_is_refused_but_a_cancel_and_a_spool_command(world: &World, print_id: PrintId) {
    for actor in [agent_actor(print_id), Actor::Operator, Actor::System] {
        for action in refused_while_waiting(&actor) {
            let kind = action.kind();
            let outcome = world
                .request(print_id, action)
                .expect("the request is recorded");
            assert_eq!(
                outcome.record.decision,
                waiting_at_the_first_change(),
                "{actor:?}'s {kind:?} was not refused for the change"
            );
            assert_eq!(outcome.executed, None, "{kind:?} reached the printer");
        }
    }
    let red = register(world, "Red", 1_000.0);
    assert_eq!(
        load(world, red.id, 0).decision,
        PolicyDecision::Accepted,
        "loading the next colour was refused"
    );
    // The printer refuses the cancel, so the print goes on to carry on: what
    // is asserted is the decision, which no planned change refused.
    world.printer.fails(
        PrinterMethod::Cancel,
        PrinterError::Unreachable {
            detail: "the journey keeps the print going".to_owned(),
        },
    );
    let cancel = world
        .request(
            print_id,
            PrintAction::Cancel {
                reason: "a person abandons the print".to_owned(),
                actor: Actor::Operator,
            },
        )
        .expect("the cancel is recorded");
    world.printer.heals();
    assert_eq!(cancel.record.decision, PolicyDecision::Accepted);
}

/// The print carried on from the first change once, and that was checked at
/// once, with a frame, naming the colour expected, its layer and the colour
/// before; the growth was told to start again before that check's image.
fn checked_once_after_the_change(
    world: &World,
    print_id: PrintId,
    checks_before: usize,
    images: &RecordingImages,
) {
    let resumed = of_print::<FilamentChangePlannedResumedPayload>(world, print_id);
    assert_eq!(resumed.len(), 1, "{resumed:?}");
    assert_eq!(resumed[0].1.change, change(0));
    assert_eq!(resumed[0].1.previous_color.as_deref(), Some("#FFFFFF"));
    assert_eq!(resumed[0].1.waited_s, 40 * 60);
    let turns = world.agent.turns();
    assert_eq!(turns.len(), checks_before + 1);
    let turn = turns.last().expect("the check after the change ran");
    assert_eq!(
        turn.event.kind().as_str(),
        FilamentChangePlannedResumedPayload::KIND
    );
    assert!(
        turn.image_path.is_some(),
        "the check after the change had no frame"
    );
    let noticed = turn
        .situation
        .noticed
        .clone()
        .expect("the check's situation says what happened");
    for named in ["#FF0000", "layer 3", "#FFFFFF"] {
        assert!(noticed.contains(named), "{noticed} does not name {named}");
    }
    assert_eq!(
        images
            .asked()
            .iter()
            .rev()
            .take(2)
            .cloned()
            .collect::<Vec<_>>(),
        vec![Asked::CheckImage, Asked::RestartGrowth(print_id)],
        "the growth was not measured afresh from the check after the change"
    );
}

/// `Obico`'s notification arriving first — `OctoPrint` paused, the job at the
/// change's own offset, no time yet passed — reaches the change on it before
/// the clock moves: the pause is refused there and then, and nothing checks
/// the paused print until it carries on, when it is resumed and checked once.
#[test]
fn a_signal_with_the_job_paused_at_the_change_reaches_it_at_once() {
    let (world, _, _) = colour_world();
    let print_id = printing(&world);
    let checks_before = world.agent.turns().len();
    world.printer.extrudes(
        FILE,
        line_offset("M600", 0),
        vec![on_tool(0, fed_before(0))],
    );
    world.printer.reports_state(PrinterState::Paused);
    world.printer.reports_job(Some(FILE), PrinterState::Paused);
    world
        .printer
        .reports_position(Some(line_offset("M600", 0)), Some(SIZE));
    let signalled_at = world.clock.now();

    world
        .handle(obico_filament_change(&world))
        .expect("the notification is handled");

    let reached = of_print::<FilamentChangePlannedReachedPayload>(&world, print_id);
    assert_eq!(reached.len(), 1, "{reached:?}");
    assert_eq!(reached[0].1.trigger, PlannedChangeTrigger::Signal);
    assert_eq!(reached[0].0.received_at, signalled_at);
    assert_eq!(world.clock.now(), signalled_at);
    let pause = world
        .request(
            print_id,
            PrintAction::Pause {
                reason: "the agent saw it stop".to_owned(),
                actor: agent_actor(print_id),
            },
        )
        .expect("the pause is recorded");
    assert_eq!(pause.record.decision, waiting_at_the_first_change());
    assert_eq!(
        world.agent.turns().len(),
        checks_before,
        "the signal claimed a turn"
    );

    for _ in 0..10 {
        world.clock.advance(120);
        assert!(watch(&world).is_empty());
        assert!(observe(&world).is_empty(), "the paused print was checked");
    }
    assert_eq!(world.agent.turns().len(), checks_before);

    printing_at(&world, carried_on_from(0));
    let check = watch(&world);
    assert_eq!(check.len(), 1, "carrying on did not claim one check");
    run(&world, check);
    assert_eq!(
        of_print::<FilamentChangePlannedResumedPayload>(&world, print_id).len(),
        1
    );
    assert_eq!(world.agent.turns().len(), checks_before + 1);
    world.journal.assert_no_violations();
}

/// A job read paused at a planned change with no signal yet — `OctoPrint`
/// paused there, as at an `M601` — is not reached by that first read, so a
/// signal arriving with no time passed reaches it as the signal; nor is the
/// paused print checked while it stands. Once it has stood there half a
/// minute with no signal, its position alone reaches the change.
#[test]
fn a_job_paused_at_a_planned_change_is_reached_by_position_only_once_it_has_stood() {
    let (world, _, _) = colour_world();
    let print_id = printing(&world);
    let checks_before = world.agent.turns().len();
    world.printer.extrudes(
        FILE,
        line_offset("M600", 0),
        vec![on_tool(0, fed_before(0))],
    );
    world.printer.reports_state(PrinterState::Paused);
    world.printer.reports_job(Some(FILE), PrinterState::Paused);
    world
        .printer
        .reports_position(Some(line_offset("M600", 0)), Some(SIZE));

    world.clock.advance(120);
    assert!(observe(&world).is_empty(), "the paused print was checked");
    assert!(watch(&world).is_empty());
    assert!(
        of_print::<FilamentChangePlannedReachedPayload>(&world, print_id).is_empty(),
        "the first read paused at the change reached it before the signal could"
    );
    assert_eq!(world.agent.turns().len(), checks_before);

    world.clock.advance(30);
    assert!(watch(&world).is_empty());
    let reached = of_print::<FilamentChangePlannedReachedPayload>(&world, print_id);
    assert_eq!(reached.len(), 1, "{reached:?}");
    assert_eq!(reached[0].1.trigger, PlannedChangeTrigger::Position);
    assert_eq!(reached[0].1.change, change(0));
    assert_eq!(world.agent.turns().len(), checks_before);
    world.journal.assert_no_violations();
}

/// A filament change signalled where the file plans none — a runout — is
/// handled as every unplanned one is: its turn runs, and its accounting record
/// names no planned change.
#[test]
fn a_filament_change_at_no_planned_offset_keeps_todays_turn() {
    let (world, _, _) = colour_world();
    let print_id = printing(&world);
    let checks_before = world.agent.turns().len();
    let mid_layer = line_offset("M73 C0", 0);
    world
        .printer
        .extrudes(FILE, mid_layer, vec![on_tool(0, 20.0)]);
    world.printer.reports_state(PrinterState::Paused);
    world.printer.reports_job(Some(FILE), PrinterState::Paused);
    world.printer.reports_position(Some(mid_layer), Some(SIZE));

    world
        .handle(obico_filament_change(&world))
        .expect("the notification is handled");

    assert!(of_print::<FilamentChangePlannedReachedPayload>(&world, print_id).is_empty());
    let turns = world.agent.turns();
    assert_eq!(
        turns.len(),
        checks_before + 1,
        "the runout's turn did not run"
    );
    assert_eq!(
        turns
            .last()
            .map(|turn| turn.event.kind().as_str().to_owned()),
        Some("obico_printer_notification".to_owned())
    );
    let signalled = every_of::<FilamentChangeSignalledPayload>(&world, print_id);
    assert_eq!(signalled.len(), 1);
    assert_eq!(signalled[0].planned_change, None);
    let unplanned =
        printobserver_types::serde_json::to_value(&signalled[0]).expect("the signal serializes");
    assert!(unplanned.get("planned_change").is_none(), "{unplanned}");
}

/// A two-change print with a spool loaded after each change charges each spool
/// exactly its own segment, as the index measures it, and holds nothing back.
#[test]
fn each_colour_is_charged_to_the_spool_loaded_for_it() {
    let (world, white, _) = colour_world();
    let red = register(&world, "Red", 1_000.0);
    let blue = register(&world, "Blue", 1_000.0);
    let print_id = printing(&world);
    for (nth, next) in [(0, &red), (1, &blue)] {
        let fed = fed_before(nth);
        world
            .printer
            .extrudes(FILE, line_offset("M600", nth), vec![on_tool(0, fed)]);
        // `OctoPrint` has sent the prime and the unretraction after the
        // change before the firmware holds it: 1.1 mm the spool feeding the
        // next colour feeds.
        let queued = line_offset(";TYPE:External perimeter", 2 * nth + 2);
        world
            .printer
            .extrudes(FILE, queued, vec![on_tool(0, fed + 1.1)]);
        printing_at(&world, queued);
        assert!(watch(&world).is_empty());
        assert_eq!(load(&world, next.id, 0).decision, PolicyDecision::Accepted);
        printing_at(&world, carried_on_from(nth));
        run(&world, watch(&world));
    }
    finishes(&world);

    for (spool, index) in [(&white, 0), (&red, 1), (&blue, 2)] {
        let charged: f64 = crate::shelf::charges_to(&world, spool.id)
            .iter()
            .filter(|charge| {
                matches!(charge.charge_key, ChargeKey::Print { print_id: of, .. } if of == print_id)
            })
            .map(|charge: &FilamentConsumedPayload| charge.length_mm)
            .sum();
        assert_close(charged, segment(index).filament_mm);
        let now = crate::shelf::spool_now(&world, spool.id);
        assert_close(1_000.0 - now.remaining_g, grams(&now, charged));
    }
    let pending = block_on(world.core.pending_charges(None)).expect("the pends are read");
    assert!(pending.is_empty(), "{pending:?}");
}

/// The context of a print waiting at a planned change names the change it
/// waits at and the colour now expected; the context of a print whose file
/// plans none carries none of the four facts about changes.
#[test]
fn the_context_names_the_change_waited_at_and_a_plain_file_names_none() {
    let (world, _, _) = colour_world();
    let print_id = printing(&world);
    world.printer.extrudes(
        FILE,
        line_offset("M600", 0),
        vec![on_tool(0, fed_before(0))],
    );
    printing_at(&world, past_change(0));
    assert!(watch(&world).is_empty());

    let plan = world
        .context(print_id)
        .expect("the context reads")
        .slicer
        .expect("the plan is answered");
    assert_eq!(plan.change_waiting, Some(change(0)));
    assert_eq!(plan.expected_color.as_deref(), Some("#FF0000"));
    assert_eq!(plan.next_change, Some(change(1)));

    let other = spool_world();
    other.printer.holds_gcode(
        FILE,
        "M83\n;LAYER_CHANGE\n;Z:0.2\nG1 X0 Y0\nG1 X10 Y0 E1\nM73 P50 R1\n",
    );
    let other_print = start(&other);
    running_at(&other, 30);
    let context = printobserver_types::serde_json::to_value(
        other.context(other_print).expect("the context reads"),
    )
    .expect("the context serializes");
    let slicer = context.get("slicer").expect("the plan is answered");
    for field in [
        "expected_color",
        "next_change",
        "minutes_to_next_change",
        "change_waiting",
    ] {
        assert!(slicer.get(field).is_none(), "{field} is in {slicer}");
    }
}

/// A two-colour file starts when its first spool holds segment 0's grams and
/// their buffer alone, and is refused naming segment 0 when it holds less.
#[test]
fn the_first_spool_is_checked_against_segment_zero_alone() {
    let world = spool_world();
    world.printer.holds_file(facts(Some(fed_before(2))));
    world.printer.holds_gcode(FILE, TWO_COLOUR);
    let probe = register(&world, "Probe", 1_000.0);
    let first = grams(&probe, segment(0).filament_mm);
    let buffer = printobserver_core::buffer_g(first, &world.config.spools);
    let whole = grams(&probe, fed_before(2));
    let whole_buffer = printobserver_core::buffer_g(whole, &world.config.spools);
    assert!(
        first + buffer < whole + whole_buffer,
        "a spool holding segment 0 would hold the whole print"
    );

    let enough = loaded_spool(&world, "Enough", first + buffer + 0.001);
    let started = request_start(&world, Actor::Operator, false);
    assert_eq!(started.record.decision, PolicyDecision::Accepted);
    let segments = &checked_for(&world, &started).segments;
    assert_eq!(segments.len(), 3);
    assert_eq!(segments[0].spool_id, Some(enough.id));
    assert_eq!(segments[0].color.as_deref(), Some("#FFFFFF"));
    assert_close(
        segments[0].required_g.expect("converted on the spool"),
        first,
    );
    assert_eq!(segments[1].spool_id, None);
    assert_eq!(segments[1].required_g, None);
    assert_close(segments[2].filament_mm, segment(2).filament_mm);

    let short_world = spool_world();
    short_world.printer.holds_file(facts(Some(fed_before(2))));
    short_world.printer.holds_gcode(FILE, TWO_COLOUR);
    let short = loaded_spool(&short_world, "Short", first + buffer - 0.001);
    let refused = request_start(&short_world, Actor::Operator, false);
    let PolicyDecision::Rejected(RejectionReason::InsufficientFilament {
        spool_id, segment, ..
    }) = &refused.record.decision
    else {
        panic!("expected a shortfall, found {:?}", refused.record.decision);
    };
    assert_eq!((*spool_id, *segment), (short.id, Some(0)));
}

/// A manifest naming a spool for a later segment is refused naming that
/// segment when the spool is short of it.
#[test]
fn a_spool_named_for_a_later_segment_is_checked_against_it() {
    let world = spool_world();
    world.printer.holds_file(facts(Some(fed_before(2))));
    world.printer.holds_gcode(FILE, TWO_COLOUR);
    let _white = loaded_spool(&world, "White", 1_000.0);
    let red = register(&world, "Red", 1_000.0);
    let need = grams(&red, segment(1).filament_mm);
    let thin = register(&world, "Thin", need / 2.0);
    let mut start = start_action(Actor::Operator, false);
    if let PrintAction::StartPrint { manifest, .. } = &mut start {
        manifest.segment_spools = vec![
            SegmentSpool {
                segment: 1,
                spool_id: red.id,
            },
            SegmentSpool {
                segment: 2,
                spool_id: thin.id,
            },
        ];
    }
    world.printer.reports_state(PrinterState::Operational);
    world.printer.reports_job(None, PrinterState::Operational);
    let refused = world
        .request(PrintId::new(), start)
        .expect("the start is recorded");

    let PolicyDecision::Rejected(RejectionReason::InsufficientFilament {
        spool_id,
        segment: refused_segment,
        ..
    }) = &refused.record.decision
    else {
        panic!("expected a shortfall, found {:?}", refused.record.decision);
    };
    assert_eq!((*spool_id, *refused_segment), (thin.id, Some(2)));
    let checked = checked_for(&world, &refused);
    assert_eq!(checked.verdict, PreflightVerdict::Insufficient);
    assert_eq!(checked.segments[1].spool_id, Some(red.id));
    assert_eq!(checked.segments[2].spool_id, Some(thin.id));
}

/// A file planning no change is checked as a whole, as it always was: its
/// check records no segment, and a spool holding segment-sized grams of it is
/// short.
#[test]
fn a_file_planning_no_change_is_checked_as_a_whole() {
    let world = spool_world();
    world.printer.holds_file(facts(Some(fed_before(2))));
    world
        .printer
        .holds_gcode(FILE, "M83\n;LAYER_CHANGE\n;Z:0.2\nG1 X10 Y0 E62.6\n");
    let probe = register(&world, "Probe", 1_000.0);
    let first = grams(&probe, segment(0).filament_mm);
    let buffer = printobserver_core::buffer_g(first, &world.config.spools);
    let _short = loaded_spool(&world, "Segment Sized", first + buffer + 0.001);

    let refused = request_start(&world, Actor::Operator, false);
    let PolicyDecision::Rejected(RejectionReason::InsufficientFilament { segment, .. }) =
        &refused.record.decision
    else {
        panic!("expected a shortfall, found {:?}", refused.record.decision);
    };
    assert_eq!(*segment, None);
    assert!(checked_for(&world, &refused).segments.is_empty());
}

/// A supervisor restarted while the printer waits at a planned change reads
/// that back as it starts: nothing checks the print, a pause is refused naming
/// the change, and carrying on is still recorded and checked once.
#[test]
fn a_restart_while_the_printer_waits_leaves_it_waiting() {
    let (mut world, _, _) = colour_world();
    let print_id = printing(&world);
    world.printer.extrudes(
        FILE,
        line_offset("M600", 0),
        vec![on_tool(0, fed_before(0))],
    );
    printing_at(&world, past_change(0));
    assert!(watch(&world).is_empty());

    world.restart();
    block_on(world.core.restore_planned_changes()).expect("the history is read back");
    let checks_before = world.agent.turns().len();

    world.clock.advance(120);
    assert!(observe(&world).is_empty(), "the waiting print was checked");
    let pause = world
        .request(
            print_id,
            PrintAction::Pause {
                reason: "the operator wants it stopped".to_owned(),
                actor: Actor::Operator,
            },
        )
        .expect("the pause is recorded");
    assert_eq!(pause.record.decision, waiting_at_the_first_change());

    printing_at(&world, carried_on_from(0));
    let check = watch(&world);
    assert_eq!(
        check.len(),
        1,
        "carrying on was not checked after the restart"
    );
    run(&world, check);
    assert_eq!(
        of_print::<FilamentChangePlannedResumedPayload>(&world, print_id).len(),
        1
    );
    assert_eq!(world.agent.turns().len(), checks_before + 1);
}

/// A supervisor restarted while the print runs towards a change it has not
/// reached builds the print's index from the first read of its job, so the
/// job freezing at the change's offset — with no check run since the restart
/// and no webhook at all — reaches it once by position: no turn runs through
/// the wait, and every actor's pause, resume and adjustment is refused naming
/// it.
#[test]
fn a_restart_before_a_change_still_reaches_it_by_position() {
    let (mut world, _, _) = colour_world();
    let print_id = printing(&world);
    world.restart();
    block_on(world.core.restore_planned_changes()).expect("the history is read back");
    let checks_before = world.agent.turns().len();
    world.printer.extrudes(
        FILE,
        line_offset("M600", 0),
        vec![on_tool(0, fed_before(0))],
    );

    printing_at(&world, past_change(0));
    assert!(
        watch(&world).is_empty(),
        "reaching the change claimed a turn"
    );
    for _ in 0..5 {
        world.clock.advance(120);
        assert!(watch(&world).is_empty(), "the watch claimed a turn");
        assert!(observe(&world).is_empty(), "the waiting print was checked");
    }

    let reached = of_print::<FilamentChangePlannedReachedPayload>(&world, print_id);
    assert_eq!(reached.len(), 1, "{reached:?}");
    assert_eq!(reached[0].1.change, change(0));
    assert_eq!(reached[0].1.trigger, PlannedChangeTrigger::Position);
    assert_eq!(
        world.agent.turns().len(),
        checks_before,
        "a turn ran while it waited"
    );
    let signalled = every_of::<FilamentChangeSignalledPayload>(&world, print_id);
    assert_eq!(signalled.len(), 1, "{signalled:?}");
    assert_eq!(signalled[0].planned_change, Some(0));
    for actor in [agent_actor(print_id), Actor::Operator, Actor::System] {
        for action in refused_while_waiting(&actor) {
            let kind = action.kind();
            let outcome = world
                .request(print_id, action)
                .expect("the request is recorded");
            assert_eq!(
                outcome.record.decision,
                waiting_at_the_first_change(),
                "{actor:?}'s {kind:?} was not refused after the restart"
            );
        }
    }
}
