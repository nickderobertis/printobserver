//! The slicer's plan the context answers, through the real core over its port
//! fakes: when it is answered at all, what a failed read or a refused record
//! leaves, how a change made through this program stands beside what the file
//! set, and how many indexes the core keeps.
//!
//! The printer fake streams each file's G-code into the core's own sink in
//! pieces that cut its lines, and counts every whole-file read, so how often a
//! file was read is read off the port rather than inferred.

use printobserver_core::kinds::GcodeIndexedPayload;
use printobserver_core::records::{InterventionOutcome, PrintAction};
use printobserver_core::store::PrintStore as _;
use printobserver_core::{Actor, PrintRecord, SetValue};
use printobserver_printer_api::{PrinterError, PrinterState};
use printobserver_types::{EventPayload as _, PrintId};

use crate::fakes::PrinterMethod;
use crate::world::World;

/// The print's file, as every journey here names it.
const FILE: &str = "benchy.gcode";

/// Two layers of a part: the flow set to one, the fan off, then on, then the
/// flow narrowed at the second layer.
const PLAN: &str = "\
M83
M221 S100
M107
;LAYER_CHANGE
;Z:0.2
;TYPE:Perimeter
G1 X10 Y10
G1 X20 Y10 E1
M106 S255
;LAYER_CHANGE
;Z:0.4
G1 X20 Y20 E1
M221 S95
G1 X10 Y20 E1
";

/// The offset just past the first line carrying `needle`.
fn past(needle: &str) -> i64 {
    let at = PLAN.find(needle).expect("the plan carries the line");
    let end = PLAN[at..]
        .find('\n')
        .map_or(PLAN.len(), |newline| at + newline + 1);
    i64::try_from(end).expect("a small offset")
}

/// A world printing the plan's file, its job at a position.
fn printing_at(position: Option<i64>) -> (World, PrintRecord) {
    let world = World::new();
    world.printer.holds_gcode(FILE, PLAN);
    world
        .printer
        .reports_job(Some(FILE), PrinterState::Printing);
    world.printer.reports_state(PrinterState::Printing);
    world
        .printer
        .reports_position(position, Some(i64::try_from(PLAN.len()).expect("small")));
    let print = world.open_print(7);
    (world, print)
}

/// How many indexes the core recorded building for a print.
fn builds(world: &World, print_id: PrintId) -> usize {
    world
        .store
        .events_of(print_id)
        .iter()
        .filter(|event| event.kind().as_str() == GcodeIndexedPayload::KIND)
        .count()
}

/// A fan change the operator asks for.
fn fan(percent: f64, duration_s: Option<i64>) -> PrintAction {
    PrintAction::SetFanPercent {
        percent,
        duration_s,
        reason: "a journey sets the fan".to_owned(),
        actor: Actor::Operator,
    }
}

/// A flow change the operator asks for.
fn flow(factor: f64) -> PrintAction {
    PrintAction::SetFlowrateFactor {
        factor,
        duration_s: None,
        reason: "a journey sets the flow".to_owned(),
        actor: Actor::Operator,
    }
}

/// The plan is answered for an open print whose file the printer is running
/// at a known position, and for nothing else: a job reporting no position, a
/// job of another file and a print that is over each answer none, and none of
/// them reads the file.
#[test]
fn the_plan_is_answered_only_for_the_print_the_running_job_is() {
    let (world, print) = printing_at(None);
    let context = world.context(print.id).expect("the context reads");
    assert_eq!(
        context.slicer, None,
        "a job reporting no position was planned"
    );

    world
        .printer
        .reports_position(Some(past("M106 S255")), None);
    world.printer.holds_gcode("other.gcode", PLAN);
    world
        .printer
        .reports_job(Some("other.gcode"), PrinterState::Printing);
    let context = world.context(print.id).expect("the context reads");
    assert_eq!(
        context.slicer, None,
        "another file's job was planned for this print"
    );
    assert_eq!(world.printer.gcode_reads("other.gcode"), 0);

    world
        .printer
        .reports_job(Some(FILE), PrinterState::Printing);
    let context = world.context(print.id).expect("the context reads");
    assert_eq!(
        context.slicer.and_then(|plan| plan.layer),
        Some(1),
        "the print's own running job was not planned"
    );

    world
        .printer
        .reports_job(Some(FILE), PrinterState::Operational);
    let context = world.context(print.id).expect("the context reads");
    assert!(
        context.print.ended_at.is_some(),
        "the job being over did not end the print"
    );
    assert_eq!(context.slicer, None, "a print that is over was planned");
    assert_eq!(world.printer.gcode_reads(FILE), 1);
}

/// A read the printer fails builds nothing and records no build, and the
/// context is answered without a plan; the next read builds the index once,
/// and no read after it touches the file.
#[test]
fn a_failed_read_builds_nothing_and_the_next_read_builds_once() {
    let (world, print) = printing_at(Some(past("G1 X20 Y10 E1")));
    world.printer.fails(
        PrinterMethod::ReadGcode,
        PrinterError::Unreachable {
            detail: "the link dropped".to_owned(),
        },
    );
    let context = world.context(print.id).expect("the context reads");
    assert_eq!(context.slicer, None);
    assert_eq!(builds(&world, print.id), 0);

    world.printer.heals();
    let planned = world
        .context(print.id)
        .expect("the context reads")
        .slicer
        .expect("the healed read planned the print");
    assert_eq!(planned.layer, Some(1));
    world.context(print.id).expect("the context reads");
    assert_eq!(world.printer.gcode_reads(FILE), 2);
    assert_eq!(builds(&world, print.id), 1);
}

/// A store refusing the record of a build leaves the index built: the plan is
/// answered, and the next read uses it rather than reading the file again.
#[test]
fn the_plan_stands_when_the_store_refuses_the_record_of_its_build() {
    let (world, print) = printing_at(Some(past("G1 X20 Y10 E1")));
    world.store.refuses_appends_for(
        print.id,
        printobserver_core::store::StoreError::Database {
            detail: "the disk is full".to_owned(),
        },
    );
    let context = world.context(print.id).expect("the context reads");
    assert_eq!(context.slicer.and_then(|plan| plan.layer), Some(1));
    assert_eq!(builds(&world, print.id), 0);

    world.store.heals();
    let context = world.context(print.id).expect("the context reads");
    assert_eq!(context.slicer.and_then(|plan| plan.layer), Some(1));
    assert_eq!(
        world.printer.gcode_reads(FILE),
        1,
        "the built index was not reused"
    );
}

/// The latest flow change made through this program stands beside what the
/// file set until the file sets the flow again; a change made before any
/// position was read is placed at the file's start, so whatever the file has
/// set since takes over from it.
#[test]
fn a_change_stands_beside_the_files_until_the_file_sets_it_again() {
    let (world, print) = printing_at(Some(past("G1 X20 Y10 E1")));
    world
        .request(print.id, fan(60.0, None))
        .expect("the fan change is recorded");
    let plan = world
        .context(print.id)
        .expect("the context reads")
        .slicer
        .expect("the print is planned");
    assert_eq!(
        plan.fan_percent,
        Some(SetValue {
            file: Some(0.0),
            changed: None
        }),
        "a change made before any position was read stood over what the file set since"
    );

    world
        .request(print.id, flow(1.02))
        .expect("the flow change is recorded");
    world
        .request(print.id, flow(1.05))
        .expect("the flow change is recorded");
    world
        .request(print.id, fan(70.0, None))
        .expect("the fan change is recorded");
    let plan = world
        .context(print.id)
        .expect("the context reads")
        .slicer
        .expect("the print is planned");
    assert_eq!(
        (plan.flow_factor, plan.fan_percent),
        (
            Some(SetValue {
                file: Some(1.0),
                changed: Some(1.05)
            }),
            Some(SetValue {
                file: Some(0.0),
                changed: Some(70.0)
            })
        ),
        "the latest changes do not stand beside the file's"
    );

    world.printer.reports_position(Some(past("M221 S95")), None);
    let plan = world
        .context(print.id)
        .expect("the context reads")
        .slicer
        .expect("the print is planned");
    assert_eq!(
        (plan.flow_factor, plan.fan_percent),
        (
            Some(SetValue {
                file: Some(0.95),
                changed: None
            }),
            Some(SetValue {
                file: Some(100.0),
                changed: None
            })
        ),
        "the file's later commands did not take over from the changes"
    );
}

/// A bounded change stands while its intervention does, and no longer once
/// the intervention has expired and put the prior value back.
#[test]
fn a_bounded_change_stands_until_its_intervention_expires() {
    let (world, print) = printing_at(Some(past("G1 X20 Y10 E1")));
    world.context(print.id).expect("the context reads");
    let outcome = world
        .request(print.id, fan(80.0, Some(60)))
        .expect("the fan change is recorded");
    let intervention = outcome.intervention.expect("a bounded change opens one");
    let plan = world
        .context(print.id)
        .expect("the context reads")
        .slicer
        .expect("the print is planned");
    assert_eq!(plan.fan_percent.and_then(|set| set.changed), Some(80.0));

    world.clock.advance(61);
    world.wait_until("the intervention settles", || {
        world
            .store
            .intervention(intervention.id)
            .is_some_and(|held| held.outcome != InterventionOutcome::StillActive)
    });
    let plan = world
        .context(print.id)
        .expect("the context reads")
        .slicer
        .expect("the print is planned");
    assert_eq!(
        plan.fan_percent.and_then(|set| set.changed),
        None,
        "an expired change still stands"
    );
}

/// The core keeps the indexes of its sixteen most recent prints: building a
/// seventeenth lets go of another, keeps the one it just built, and a print
/// let go of is read and built again when it runs once more.
#[test]
fn the_core_keeps_a_bounded_number_of_indexes() {
    let world = World::new();
    world.printer.reports_state(PrinterState::Printing);
    let mut prints = Vec::new();
    for number in 0..17 {
        let file = format!("part-{number}.gcode");
        world.printer.holds_gcode(&file, PLAN);
        world
            .printer
            .reports_job(Some(&file), PrinterState::Printing);
        world
            .printer
            .reports_position(Some(past("G1 X20 Y10 E1")), None);
        let print = printobserver_core::block_on(
            world
                .store
                .open_print(Some(100 + number), Some(file.clone())),
        )
        .expect("the store opens a print");
        assert!(
            world
                .context(print.id)
                .expect("the context reads")
                .slicer
                .is_some(),
            "{file} was not planned"
        );
        prints.push((print, file));
    }
    let reads = |file: &str| world.printer.gcode_reads(file);
    assert!(
        prints.iter().all(|(_, file)| reads(file) == 1),
        "a print was read more than once on its way in"
    );

    let (last, last_file) = &prints[16];
    world.context(last.id).expect("the context reads");
    assert_eq!(reads(last_file), 1, "the newest index was let go of");

    let rebuilt: Vec<&str> = prints[..16]
        .iter()
        .filter(|(print, file)| {
            world
                .printer
                .reports_job(Some(file), PrinterState::Printing);
            world.context(print.id).expect("the context reads");
            reads(file) == 2
        })
        .map(|(_, file)| file.as_str())
        .collect();
    assert!(
        !rebuilt.is_empty(),
        "seventeen prints were all held, past the bound"
    );
}
